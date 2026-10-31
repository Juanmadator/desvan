//! Puente entre los módulos del núcleo (treemap, limpieza, duplicados) y la interfaz.

use crate::state::ScanData;
use desvan_core::layout::{self, LayoutOpts, Metric};
use desvan_core::{NodeId, Progress, Tree};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Treemap de un nodo como bytes: el formato de `Layout::to_bytes` y, a continuación,
/// las etiquetas de los rectángulos con sitio para texto:
/// `u32` n, y n veces `u32` índice, `u64` tamaño, `u16` longitud y el nombre en UTF-8.
pub fn layout_bytes(t: &Tree, id: NodeId, w: f32, h: f32, metric: Option<&str>, min_area: Option<f32>) -> Vec<u8> {
    let metric = metric.and_then(Metric::parse).unwrap_or(Metric::Alloc);
    let opts = LayoutOpts { metric, min_area: min_area.unwrap_or(4.0).max(1.0), ..Default::default() };
    let w = w.clamp(1.0, 16384.0);
    let h = h.clamp(1.0, 16384.0);
    let lay = layout::layout(t, id, w, h, &opts);
    let mut out = lay.to_bytes(w, h);
    let cands: Vec<u32> = lay.label_candidates(48.0, 18.0).into_iter().filter(|&i| lay.flags(i as usize) & layout::rflags::REST == 0).collect();
    out.extend_from_slice(&(cands.len() as u32).to_le_bytes());
    for i in cands {
        let node = lay.ids[i as usize];
        let name = t.name(node);
        let name = &name.as_bytes()[..name.len().min(400)];
        let value = if metric == Metric::Alloc { t.alloc[node as usize] } else { t.size[node as usize] };
        out.extend_from_slice(&i.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
    }
    out
}

#[derive(Serialize)]
struct JunkNode {
    id: u32,
    path: String,
    alloc: u64,
}

#[derive(Serialize)]
struct JunkDto {
    rule: String,
    title: String,
    description: String,
    risk: String,
    bytes: u64,
    nodes: Vec<JunkNode>,
}

pub fn junk_json(t: &Tree, root_path: &str) -> serde_json::Value {
    let hits = desvan_core::junk::find_junk(t, root_path);
    let dto: Vec<JunkDto> = hits
        .into_iter()
        .map(|h| JunkDto {
            rule: h.rule.to_string(),
            title: h.title,
            description: h.description,
            risk: format!("{:?}", h.risk).to_lowercase(),
            bytes: h.bytes,
            nodes: h.nodes.iter().map(|&n| JunkNode { id: n, path: t.path(n, root_path), alloc: t.alloc[n as usize] }).collect(),
        })
        .collect();
    serde_json::to_value(dto).unwrap_or_default()
}

#[derive(Serialize, Clone)]
struct DupNode {
    id: u32,
    path: String,
    mtime: i64,
}

#[derive(Serialize, Clone)]
struct DupDto {
    size: u64,
    hash: String,
    wasted: u64,
    nodes: Vec<DupNode>,
}

#[derive(Serialize, Clone)]
struct DupProgress {
    done: u64,
    total: u64,
    groups: u64,
    wasted: u64,
    finished: bool,
}

/// Busca duplicados bajo `id` emitiendo `dupes-group` según se confirman y `dupes-progress`.
pub fn run_dupes(app: AppHandle, d: Arc<ScanData>, id: NodeId, min_size: u64, cancel: Arc<AtomicBool>) {
    let progress = Arc::new(Progress::default());
    let groups = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let wasted = Arc::new(std::sync::atomic::AtomicU64::new(0));
    // vigía: progreso cada 150 ms y traslado de la cancelación
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (app, p, c, stop, g, w) = (app.clone(), progress.clone(), cancel.clone(), stop.clone(), groups.clone(), wasted.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if c.load(Ordering::Relaxed) {
                    p.cancel.store(true, Ordering::Relaxed);
                }
                let s = p.snapshot();
                let _ = app.emit("dupes-progress", DupProgress { done: s.done, total: s.total, groups: g.load(Ordering::Relaxed), wasted: w.load(Ordering::Relaxed), finished: false });
                std::thread::sleep(Duration::from_millis(150));
            }
        });
    }
    let s = d.scan.read().unwrap();
    let t = &s.tree;
    let root_path = s.root_path.clone();
    // el análisis se limita al subárbol: se pasa la raíz del árbol pero se filtra por antepasado
    let opts = desvan_core::dupes::DupOptions { min_size, ..Default::default() };
    let in_scope = |n: NodeId| -> bool {
        let mut cur = n;
        while cur != desvan_core::NONE {
            if cur == id {
                return true;
            }
            cur = t.parent[cur as usize];
        }
        false
    };
    let emit_group = |g: &desvan_core::dupes::DupGroup| {
        let nodes: Vec<DupNode> = g.nodes.iter().filter(|&&n| in_scope(n)).map(|&n| DupNode { id: n, path: t.path(n, &root_path), mtime: t.mtime[n as usize] }).collect();
        if nodes.len() < 2 {
            return;
        }
        let w = g.size * (nodes.len() as u64 - 1);
        groups.fetch_add(1, Ordering::Relaxed);
        wasted.fetch_add(w, Ordering::Relaxed);
        let _ = app.emit("dupes-group", DupDto { size: g.size, hash: format!("{:032x}", g.hash), wasted: w, nodes });
    };
    let _ = desvan_core::dupes::find_dupes_in(t, id, &root_path, &opts, &progress, emit_group);
    stop.store(true, Ordering::Relaxed);
    let snap = progress.snapshot();
    let _ = app.emit("dupes-progress", DupProgress { done: snap.total, total: snap.total, groups: groups.load(Ordering::Relaxed), wasted: wasted.load(Ordering::Relaxed), finished: true });
}
