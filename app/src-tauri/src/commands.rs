//! Comandos que la interfaz invoca. El árbol nunca viaja entero: solo lo visible.

use crate::state::{AppState, ScanData};
use desvan_core::{flags, ntfs, NodeId, Progress, Scan, Tree, NONE};
use rayon::prelude::*;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------- unidades y permisos ----------

/// Argumentos con los que arrancó la app (p. ej. `--scan C:` tras pedir permisos).
#[tauri::command]
pub fn startup_args() -> String {
    std::env::args().skip(1).collect::<Vec<_>>().join(" ")
}

#[tauri::command]
pub fn volumes() -> Vec<desvan_core::win::VolumeEntry> {
    desvan_core::win::volumes()
}

#[derive(Serialize)]
pub struct Elevation {
    elevated: bool,
}

#[tauri::command]
pub fn elevation() -> Elevation {
    Elevation { elevated: desvan_core::win::is_elevated() }
}

/// Reinicia la app como administrador; si el usuario acepta el aviso, esta instancia se cierra.
#[tauri::command]
pub fn relaunch_admin(app: AppHandle, target: Option<String>) -> bool {
    let args = target.map(|t| format!("--scan {t}")).unwrap_or_default();
    let ok = desvan_core::win::relaunch_elevated(&args);
    if ok {
        app.exit(0);
    }
    ok
}

// ---------- escaneo ----------

#[derive(Serialize, Clone)]
pub struct ScanSummary {
    id: u32,
    root: u32,
    root_path: String,
    method: desvan_core::Method,
    stats: desvan_core::ScanStats,
    volume: Option<desvan_core::VolumeInfo>,
    nodes: usize,
    tree_bytes: usize,
    alloc: u64,
    size: u64,
    files: u32,
    dirs: u32,
}

fn summary(d: &ScanData) -> ScanSummary {
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    let r = t.root as usize;
    ScanSummary {
        id: d.id,
        root: t.root,
        root_path: s.root_path.clone(),
        method: s.method,
        stats: s.stats.clone(),
        volume: s.volume.clone(),
        nodes: t.len(),
        tree_bytes: t.heap_bytes(),
        alloc: t.alloc[r],
        size: t.size[r],
        files: t.files[r],
        dirs: t.dirs[r],
    }
}

fn categories_for(t: &Tree) -> Vec<u8> {
    let ext_cat: Vec<u8> = t.exts.iter().map(|e| desvan_core::categories::category(e)).collect();
    (0..t.len()).into_par_iter().map(|i| if t.flags[i] & flags::DIR != 0 { 255 } else { ext_cat[t.ext[i] as usize] }).collect()
}

/// Escanea una unidad ("C") o una carpeta. Usa la MFT si es NTFS y hay permisos;
/// si no, recorre las carpetas. El progreso llega por el evento `scan-progress`.
#[tauri::command]
pub fn scan_start(app: AppHandle, state: State<'_, AppState>, target: String, prefer_mft: Option<bool>) -> Res<u32> {
    let id = {
        let mut n = state.next_id.lock().unwrap();
        *n += 1;
        *n
    };
    let progress = Arc::new(Progress::default());
    *state.progress.lock().unwrap() = Some(progress.clone());

    // vigía del progreso: emite cada 80 ms mientras dure el escaneo
    {
        let app = app.clone();
        let p = progress.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(80));
            let snap = p.snapshot();
            let _ = app.emit("scan-progress", (id, &snap));
            if snap.phase == desvan_core::Phase::Done || p.cancelled() {
                break;
            }
        });
    }

    let prefer_mft = prefer_mft.unwrap_or(true);
    std::thread::Builder::new()
        .name("escaneo".into())
        .spawn(move || {
            let result = run_scan(&target, prefer_mft, &progress);
            progress.set_phase(desvan_core::Phase::Done);
            match result {
                Ok(scan) => {
                    let cats = categories_for(&scan.tree);
                    let data = Arc::new(ScanData { id, scan: RwLock::new(scan), cats: RwLock::new(cats), lower: Default::default() });
                    let st = app.state::<AppState>();
                    *st.current.write().unwrap() = Some(data.clone());
                    let _ = app.emit("scan-done", summary(&data));
                }
                Err(e) => {
                    let _ = app.emit("scan-error", (id, e));
                }
            }
        })
        .map_err(err)?;
    Ok(id)
}

use tauri::Manager;

fn run_scan(target: &str, prefer_mft: bool, progress: &Progress) -> Result<Scan, String> {
    let t = target.trim().trim_end_matches(['\\', '/']);
    let is_drive = t.len() == 2 && t.ends_with(':') || t.len() == 1;
    if is_drive {
        let letter = t.chars().next().unwrap().to_ascii_uppercase();
        let root = format!("{letter}:\\");
        let vol = desvan_core::win::volumes().into_iter().find(|v| v.letter.starts_with(letter));
        let ntfs_vol = vol.as_ref().map(|v| v.filesystem.eq_ignore_ascii_case("NTFS")).unwrap_or(false);
        if prefer_mft && ntfs_vol && desvan_core::win::is_elevated() {
            match ntfs::FileSource::open_volume(letter) {
                Ok(src) => match ntfs::scan(&src, &root, progress) {
                    Ok(mut s) => {
                        if let Some(v) = &vol {
                            s.volume = Some(desvan_core::VolumeInfo { total: v.total, free: v.free, cluster: v.cluster, filesystem: v.filesystem.clone(), label: v.label.clone() });
                        }
                        return Ok(s);
                    }
                    Err(desvan_core::Error::Cancelled) => return Err("cancelado".into()),
                    Err(e) => eprintln!("MFT no disponible ({e}); recorro carpetas"),
                },
                Err(e) => eprintln!("no se pudo abrir el volumen ({e}); recorro carpetas"),
            }
        }
        return desvan_core::walk::scan(std::path::Path::new(&root), progress).map_err(err);
    }
    desvan_core::walk::scan(std::path::Path::new(target), progress).map_err(err)
}

#[tauri::command]
pub fn scan_cancel(state: State<'_, AppState>) {
    if let Some(p) = state.progress.lock().unwrap().as_ref() {
        p.cancel.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn scan_summary(state: State<'_, AppState>) -> Res<ScanSummary> {
    Ok(summary(&state.current()?))
}

// ---------- consultas sobre el árbol ----------

#[derive(Serialize)]
pub struct Row {
    id: u32,
    name: String,
    alloc: u64,
    size: u64,
    files: u32,
    dirs: u32,
    flags: u16,
    ext: String,
    cat: u8,
    mtime: i64,
    kids: u32,
}

fn row(t: &Tree, cats: &[u8], n: NodeId) -> Row {
    let i = n as usize;
    Row {
        id: n,
        name: t.name(n).to_string(),
        alloc: t.alloc[i],
        size: t.size[i],
        files: t.files[i],
        dirs: t.dirs[i],
        flags: t.flags[i],
        ext: t.ext_name(n).to_string(),
        cat: cats[i],
        mtime: t.mtime[i],
        kids: t.child_start[i + 1] - t.child_start[i],
    }
}

#[derive(Serialize)]
pub struct NodeInfo {
    #[serde(flatten)]
    row: Row,
    path: String,
    parent: u32,
    depth: u32,
    /// fracción del padre y de la raíz
    of_parent: f64,
    of_root: f64,
}

fn valid(t: &Tree, n: u32) -> Res<()> {
    if (n as usize) < t.len() {
        Ok(())
    } else {
        Err("nodo inexistente".into())
    }
}

#[tauri::command]
pub fn node(state: State<'_, AppState>, id: u32) -> Res<NodeInfo> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    valid(t, id)?;
    let cats = d.cats.read().unwrap();
    let p = t.parent[id as usize];
    let pa = if p == NONE { t.alloc[id as usize] } else { t.alloc[p as usize] };
    Ok(NodeInfo {
        row: row(t, &cats, id),
        path: t.path(id, &s.root_path),
        parent: p,
        depth: t.depth(id),
        of_parent: t.alloc[id as usize] as f64 / pa.max(1) as f64,
        of_root: t.alloc[id as usize] as f64 / t.alloc[t.root as usize].max(1) as f64,
    })
}

#[tauri::command]
pub fn children(state: State<'_, AppState>, id: u32, offset: Option<u32>, limit: Option<u32>) -> Res<Vec<Row>> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    valid(t, id)?;
    let cats = d.cats.read().unwrap();
    let kids = t.children(id);
    let o = offset.unwrap_or(0) as usize;
    let l = limit.unwrap_or(500) as usize;
    Ok(kids.iter().skip(o).take(l).filter(|&&c| t.flags[c as usize] & flags::DELETED == 0 && (t.alloc[c as usize] > 0 || t.size[c as usize] > 0 || t.flags[c as usize] & flags::DIR != 0)).map(|&c| row(t, &cats, c)).collect())
}

#[derive(Serialize)]
pub struct Crumb {
    id: u32,
    name: String,
}

#[tauri::command]
pub fn ancestors(state: State<'_, AppState>, id: u32) -> Res<Vec<Crumb>> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    valid(t, id)?;
    Ok(t.ancestors(id).into_iter().map(|n| Crumb { id: n, name: t.name(n).to_string() }).collect())
}

/// Nodos que cuelgan de `id` (incluido), en profundidad.
fn subtree(t: &Tree, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        out.push(n);
        stack.extend_from_slice(t.children(n));
    }
    out
}

#[derive(Serialize)]
pub struct Hit {
    #[serde(flatten)]
    row: Row,
    path: String,
}

/// Busca por nombre (sin distinguir mayúsculas). Devuelve los más grandes primero.
#[tauri::command]
pub fn search(state: State<'_, AppState>, query: String, limit: Option<usize>) -> Res<Vec<Hit>> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let lower = d.lower.get_or_init(|| t.names.iter().map(|b| b.to_ascii_lowercase()).collect());
    // en minúsculas ASCII la búsqueda de no ASCII se hace aparte
    let ascii = q.is_ascii();
    let finder = q.as_bytes();
    let mut hits: Vec<NodeId> = (0..t.len() as u32)
        .into_par_iter()
        .filter(|&n| {
            let i = n as usize;
            if t.flags[i] & flags::SYNTHETIC != 0 {
                return false;
            }
            let o = t.name_off[i] as usize;
            let l = t.name_len[i] as usize;
            if ascii {
                let name = &lower[o..o + l];
                name.windows(finder.len()).any(|w| w == finder)
            } else {
                t.name(n).to_lowercase().contains(&q)
            }
        })
        .collect();
    hits.par_sort_unstable_by(|&a, &b| t.alloc[b as usize].cmp(&t.alloc[a as usize]));
    hits.truncate(limit.unwrap_or(300));
    let cats = d.cats.read().unwrap();
    Ok(hits.into_iter().map(|n| Hit { row: row(t, &cats, n), path: t.path(n, &s.root_path) }).collect())
}

/// Los archivos más grandes bajo un nodo.
#[tauri::command]
pub fn largest(state: State<'_, AppState>, id: u32, limit: Option<usize>) -> Res<Vec<Hit>> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    valid(t, id)?;
    let mut files: Vec<NodeId> = subtree(t, id).into_iter().filter(|&n| t.flags[n as usize] & (flags::DIR | flags::HARDLINK) == 0).collect();
    let k = limit.unwrap_or(200).min(files.len());
    if k == 0 {
        return Ok(Vec::new());
    }
    files.select_nth_unstable_by(k - 1, |&a, &b| t.alloc[b as usize].cmp(&t.alloc[a as usize]));
    files.truncate(k);
    files.sort_unstable_by(|&a, &b| t.alloc[b as usize].cmp(&t.alloc[a as usize]));
    let cats = d.cats.read().unwrap();
    Ok(files.into_iter().map(|n| Hit { row: row(t, &cats, n), path: t.path(n, &s.root_path) }).collect())
}

#[derive(Serialize)]
pub struct ExtStat {
    ext: String,
    cat: u8,
    alloc: u64,
    size: u64,
    files: u64,
}

#[derive(Serialize)]
pub struct CatStat {
    cat: u8,
    alloc: u64,
    files: u64,
}

#[derive(Serialize)]
pub struct ExtStats {
    exts: Vec<ExtStat>,
    cats: Vec<CatStat>,
}

/// Espacio por extensión y por categoría bajo un nodo.
#[tauri::command]
pub fn ext_stats(state: State<'_, AppState>, id: u32, limit: Option<usize>) -> Res<ExtStats> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    valid(t, id)?;
    let cats = d.cats.read().unwrap();
    let mut by_ext: Vec<(u64, u64, u64)> = vec![(0, 0, 0); t.exts.len()];
    for n in subtree(t, id) {
        let i = n as usize;
        if t.flags[i] & (flags::DIR | flags::HARDLINK) != 0 {
            continue;
        }
        let e = &mut by_ext[t.ext[i] as usize];
        e.0 += t.alloc[i];
        e.1 += t.size[i];
        e.2 += 1;
    }
    let mut cat_tot: std::collections::BTreeMap<u8, (u64, u64)> = Default::default();
    let mut exts: Vec<ExtStat> = by_ext
        .iter()
        .enumerate()
        .filter(|(_, v)| v.2 > 0)
        .map(|(i, v)| {
            let cat = desvan_core::categories::category(&t.exts[i]);
            let c = cat_tot.entry(cat).or_default();
            c.0 += v.0;
            c.1 += v.2;
            ExtStat { ext: t.exts[i].clone(), cat, alloc: v.0, size: v.1, files: v.2 }
        })
        .collect();
    drop(cats);
    exts.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
    exts.truncate(limit.unwrap_or(300));
    let mut cats: Vec<CatStat> = cat_tot.into_iter().map(|(cat, (alloc, files))| CatStat { cat, alloc, files }).collect();
    cats.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
    Ok(ExtStats { exts, cats })
}

// ---------- treemap, limpieza y duplicados ----------

#[tauri::command]
pub fn layout(state: State<'_, AppState>, id: u32, width: f32, height: f32, metric: Option<String>, min_area: Option<f32>) -> Res<tauri::ipc::Response> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    valid(t, id)?;
    let bytes = crate::bridge::layout_bytes(t, id, width, height, metric.as_deref(), min_area);
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn junk(state: State<'_, AppState>) -> Res<serde_json::Value> {
    let d = state.current()?;
    let s = d.scan.read().unwrap();
    Ok(crate::bridge::junk_json(&s.tree, &s.root_path))
}

#[tauri::command]
pub fn dupes_start(app: AppHandle, state: State<'_, AppState>, id: u32, min_size: Option<u64>) -> Res<()> {
    let d = state.current()?;
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(old) = state.dupes_cancel.lock().unwrap().replace(cancel.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    std::thread::spawn(move || crate::bridge::run_dupes(app, d, id, min_size.unwrap_or(1 << 20), cancel));
    Ok(())
}

#[tauri::command]
pub fn dupes_cancel(state: State<'_, AppState>) {
    if let Some(c) = state.dupes_cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

// ---------- acciones ----------

#[derive(Serialize)]
pub struct DeleteResult {
    deleted: Vec<u32>,
    freed: u64,
    root_alloc: u64,
}

/// Envía a la Papelera y descuenta el espacio en el árbol (sin volver a escanear).
#[tauri::command]
pub fn delete(state: State<'_, AppState>, ids: Vec<u32>) -> Res<DeleteResult> {
    let d = state.current()?;
    let paths: Vec<String> = {
        let s = d.scan.read().unwrap();
        let t = &s.tree;
        for &id in &ids {
            valid(t, id)?;
            if id == t.root || t.flags[id as usize] & (flags::SYNTHETIC | flags::METAFILE) != 0 {
                return Err(format!("«{}» no se puede borrar", t.name(id)));
            }
        }
        ids.iter().map(|&n| t.path(n, &s.root_path)).collect()
    };
    desvan_core::win::recycle(&paths)?;
    let mut s = d.scan.write().unwrap();
    let t = &mut s.tree;
    let mut freed = 0;
    for &id in &ids {
        let i = id as usize;
        if t.flags[i] & flags::DELETED != 0 {
            continue;
        }
        let (a, z, f, dd) = (t.alloc[i], t.size[i], t.files[i], t.dirs[i] + t.is_dir(id) as u32);
        let counts = t.flags[i] & flags::HARDLINK == 0;
        let mut p = t.parent[i];
        while p != NONE {
            let pi = p as usize;
            if counts {
                t.alloc[pi] -= a.min(t.alloc[pi]);
                t.size[pi] -= z.min(t.size[pi]);
            }
            t.files[pi] -= f.min(t.files[pi]);
            t.dirs[pi] -= dd.min(t.dirs[pi]);
            p = t.parent[pi];
        }
        // el nodo y su contenido quedan a cero (la interfaz los oculta)
        for n in subtree(t, id) {
            let k = n as usize;
            t.alloc[k] = 0;
            t.size[k] = 0;
            t.files[k] = 0;
            t.flags[k] |= flags::DELETED;
        }
        if counts {
            freed += a;
        }
    }
    let root_alloc = t.alloc[t.root as usize];
    Ok(DeleteResult { deleted: ids, freed, root_alloc })
}

#[tauri::command]
pub fn reveal(app: AppHandle, state: State<'_, AppState>, id: u32) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let d = state.current()?;
    let path = {
        let s = d.scan.read().unwrap();
        valid(&s.tree, id)?;
        s.tree.path(id, &s.root_path)
    };
    app.opener().reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
pub fn open_path(app: AppHandle, state: State<'_, AppState>, id: u32) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let d = state.current()?;
    let path = {
        let s = d.scan.read().unwrap();
        valid(&s.tree, id)?;
        s.tree.path(id, &s.root_path)
    };
    app.opener().open_path(path, None::<&str>).map_err(err)
}
