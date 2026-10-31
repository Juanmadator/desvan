//! Buscador de archivos duplicados sobre un árbol ya escaneado.
//!
//! 1. Candidatos: archivos normales de al menos `min_size` bytes. Fuera quedan los enlaces
//!    duros secundarios, los metadatos de NTFS, los marcadores de la nube, los enlaces y
//!    lo que no se pudo leer.
//! 2. Se agrupan por tamaño exacto.
//! 3. Huella parcial (xxh3 de los primeros y los últimos 64 KB) para partir los grupos.
//! 4. Huella completa (xxh3-128 en bloques de 1 MB) solo de lo que sigue empatado.
//!
//! Los grupos se atienden de mayor a menor espacio desperdiciado posible, con un tope de
//! archivos abiertos a la vez, y cada grupo confirmado se avisa al momento por una función
//! para que la interfaz lo vaya mostrando. Los enlaces duros de un mismo archivo nunca
//! cuentan como duplicados (misma clave en el árbol o mismo id al abrirlos).

use crate::tree::{flags, NodeId, Tree};
use crate::{Error, Phase, Progress, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Read};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use xxhash_rust::xxh3::{xxh3_128, Xxh3};

/// Bytes que se leen del principio y del final para la huella parcial.
const PART: u64 = 64 * 1024;
/// Bloque de lectura de la huella completa.
const BLOCK: usize = 1 << 20;

#[derive(Clone, Debug)]
pub struct DupOptions {
    /// Tamaño mínimo de archivo (por defecto 1 MB). Los vacíos nunca cuentan.
    pub min_size: u64,
    /// Archivos abiertos a la vez (por defecto 8, adecuado para SSD; 1–2 para discos duros).
    pub max_open: usize,
}

impl Default for DupOptions {
    fn default() -> Self {
        Self { min_size: 1 << 20, max_open: 8 }
    }
}

/// Archivos con el mismo contenido.
#[derive(Clone, Debug, Serialize)]
pub struct DupGroup {
    /// Tamaño de cada copia.
    pub size: u64,
    /// xxh3-128 del contenido (en JSON, 32 cifras hexadecimales).
    #[serde(serialize_with = "hex128")]
    pub hash: u128,
    /// Copias, la más antigua primero.
    pub nodes: Vec<NodeId>,
    /// Bytes que se liberan dejando una sola copia: `size × (copias − 1)`.
    pub wasted: u64,
}

fn hex128<S: serde::Serializer>(h: &u128, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{h:032x}"))
}

/// Busca duplicados en todo `tree` (`root_path` es la ruta de su raíz, p. ej. `C:\`).
/// Es [`find_dupes_in`] desde la raíz del árbol.
pub fn find_dupes(tree: &Tree, root_path: &str, opts: &DupOptions, progress: &Progress, on_group: impl Fn(&DupGroup) + Sync) -> Result<Vec<DupGroup>> {
    find_dupes_in(tree, tree.root, root_path, opts, progress, on_group)
}

/// Busca duplicados entre los archivos que cuelgan de `root` (incluido).
/// `root_path` sigue siendo la ruta de la raíz del ÁRBOL (para `tree.path`).
///
/// `on_group` se llama desde los hilos de trabajo con cada grupo según se confirma.
/// Progreso: `done`/`total` = bytes leídos / bytes por leer (el total se ajusta a la baja
/// cuando se descartan archivos), `files` = archivos resueltos, `bytes` = desperdicio hallado.
/// La cancelación se comprueba antes de cada archivo y cada bloque de 1 MB.
/// Devuelve los grupos ordenados de mayor a menor desperdicio.
pub fn find_dupes_in(tree: &Tree, root: NodeId, root_path: &str, opts: &DupOptions, progress: &Progress, on_group: impl Fn(&DupGroup) + Sync) -> Result<Vec<DupGroup>> {
    progress.set_phase(Phase::Starting);
    for c in [&progress.done, &progress.total, &progress.files, &progress.bytes] {
        c.store(0, Relaxed);
    }
    if (root as usize) >= tree.len() {
        progress.set_phase(Phase::Done);
        return Ok(Vec::new());
    }
    let groups = size_groups(tree, root, opts.min_size.max(1), progress);
    if progress.cancelled() {
        return Err(Error::Cancelled);
    }
    let total: u64 = groups.iter().map(|g| g.1.len() as u64 * planned_bytes(g.0)).sum();
    progress.total.store(total, Relaxed);
    progress.set_phase(Phase::Reading);

    let threads = opts.max_open.max(1);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("desvan-dupes-{i}"))
        .build()
        .map_err(|e| Error::Io(io::Error::other(e)))?;
    let job = Job { tree, root_path, progress, on_group: &on_group, found: Mutex::new(Vec::new()) };
    let next = AtomicUsize::new(0);
    // un bucle por hilo que va tomando grupos en orden; dentro de un grupo grande
    // los hilos libres ayudan (cada hilo tiene como mucho un archivo abierto)
    pool.broadcast(|_| loop {
        if progress.cancelled() {
            break;
        }
        let i = next.fetch_add(1, Relaxed);
        let Some((size, nodes)) = groups.get(i) else { break };
        job.group(*size, nodes);
    });
    if progress.cancelled() {
        return Err(Error::Cancelled);
    }
    let mut found = job.found.into_inner().unwrap_or_else(|e| e.into_inner());
    found.sort_by(|a, b| b.wasted.cmp(&a.wasted).then(b.size.cmp(&a.size)).then(a.nodes.cmp(&b.nodes)));
    progress.set_phase(Phase::Done);
    Ok(found)
}

/// Bytes que se leerán de un archivo de `size` si llega hasta el final.
fn planned_bytes(size: u64) -> u64 {
    if size <= 2 * PART {
        size
    } else {
        2 * PART + size
    }
}

/// ¿Puede ser duplicado? Archivos normales con datos locales y ruta conocida.
fn candidate(tree: &Tree, n: usize, min_size: u64) -> bool {
    const SKIP: u16 = flags::DIR | flags::HARDLINK | flags::METAFILE | flags::OFFLINE | flags::REPARSE | flags::DENIED | flags::SYNTHETIC | flags::ORPHAN;
    tree.flags[n] & SKIP == 0 && tree.size[n] >= min_size
}

/// Grupos de candidatos del mismo tamaño (2 o más) bajo `root`, del mayor desperdicio
/// posible al menor. No entra en «huérfanos» (sus rutas no existen).
fn size_groups(tree: &Tree, root: NodeId, min_size: u64, progress: &Progress) -> Vec<(u64, Vec<NodeId>)> {
    let mut c: Vec<(u64, u64, NodeId)> = Vec::new();
    let mut stack = vec![root];
    let mut seen = 0usize;
    while let Some(n) = stack.pop() {
        seen += 1;
        if seen.is_multiple_of(65_536) && progress.cancelled() {
            return Vec::new();
        }
        if tree.is_dir(n) {
            stack.extend(tree.children(n).iter().copied().filter(|&k| k != tree.orphans));
        } else if candidate(tree, n as usize, min_size) {
            c.push((tree.size[n as usize], tree.key[n as usize], n));
        }
    }
    // misma clave = mismo archivo (enlaces duros sin marcar): se queda uno
    c.par_sort_unstable_by_key(|&(s, k, n)| (k, s, n));
    c.dedup_by_key(|x| x.1);
    c.par_sort_unstable();
    let mut groups: Vec<(u64, Vec<NodeId>)> = c.chunk_by(|a, b| a.0 == b.0).filter(|g| g.len() > 1).map(|g| (g[0].0, g.iter().map(|x| x.2).collect())).collect();
    let waste = |g: &(u64, Vec<NodeId>)| g.0.saturating_mul(g.1.len() as u64 - 1);
    groups.sort_by(|a, b| waste(b).cmp(&waste(a)).then(b.0.cmp(&a.0)));
    groups
}

/// Identidad del archivo abierto (volumen, id), solo si tiene varios enlaces duros.
type Ident = Option<(u64, u64)>;

struct Job<'a, F> {
    tree: &'a Tree,
    root_path: &'a str,
    progress: &'a Progress,
    on_group: &'a F,
    found: Mutex<Vec<DupGroup>>,
}

impl<F: Fn(&DupGroup) + Sync> Job<'_, F> {
    /// Resuelve un grupo de archivos del mismo tamaño.
    fn group(&self, size: u64, nodes: &[NodeId]) {
        let p = self.progress;
        let full_needed = size > 2 * PART;
        // 1) huella parcial (y de paso la identidad para descartar enlaces duros)
        let partial: Vec<Option<(u128, Ident)>> = nodes.par_iter().with_max_len(1).map(|&n| self.partial(n, size)).collect();
        let mut tagged: Vec<(u128, NodeId, Ident)> = Vec::with_capacity(nodes.len());
        for (&n, r) in nodes.iter().zip(partial) {
            match r {
                Some((h, id)) => tagged.push((h, n, id)),
                None => {
                    self.unplan(planned_bytes(size));
                    p.files.fetch_add(1, Relaxed);
                }
            }
        }
        self.drop_same_files(&mut tagged, size);
        tagged.sort_unstable_by_key(|t| (t.0, t.1));
        for run in tagged.chunk_by(|a, b| a.0 == b.0) {
            if run.len() < 2 {
                if full_needed {
                    self.unplan(size);
                }
                p.files.fetch_add(1, Relaxed);
                continue;
            }
            let members: Vec<NodeId> = run.iter().map(|t| t.1).collect();
            if !full_needed {
                // la parcial ya cubría el archivo entero
                p.files.fetch_add(members.len() as u64, Relaxed);
                self.confirm(size, run[0].0, members);
                continue;
            }
            // 2) huella completa
            let full: Vec<Option<u128>> = members.par_iter().with_max_len(1).map(|&n| self.full(n, size)).collect();
            let mut ok: Vec<(u128, NodeId)> = members.iter().zip(full).filter_map(|(&n, h)| h.map(|h| (h, n))).collect();
            p.files.fetch_add(members.len() as u64, Relaxed);
            ok.sort_unstable();
            for same in ok.chunk_by(|a, b| a.0 == b.0) {
                if same.len() > 1 {
                    self.confirm(size, same[0].0, same.iter().map(|x| x.1).collect());
                }
            }
        }
    }

    /// Enlaces duros del mismo archivo que el árbol no marcó: se queda el primero.
    fn drop_same_files(&self, tagged: &mut Vec<(u128, NodeId, Ident)>, size: u64) {
        let before = tagged.len();
        tagged.sort_unstable_by_key(|t| (t.2, t.1));
        tagged.dedup_by(|a, b| a.2.is_some() && a.2 == b.2);
        for _ in tagged.len()..before {
            if size > 2 * PART {
                self.unplan(size);
            }
            self.progress.files.fetch_add(1, Relaxed);
        }
    }

    fn confirm(&self, size: u64, hash: u128, mut nodes: Vec<NodeId>) {
        // tras cancelar, un grupo podría estar incompleto: no se avisa
        if self.progress.cancelled() {
            return;
        }
        let t = self.tree;
        nodes.sort_by_key(|&n| (t.mtime[n as usize], n));
        let g = DupGroup { size, hash, wasted: size * (nodes.len() as u64 - 1), nodes };
        self.progress.bytes.fetch_add(g.wasted, Relaxed);
        (self.on_group)(&g);
        self.found.lock().unwrap_or_else(|e| e.into_inner()).push(g);
    }

    /// Resta del total previsto lo que ya no se va a leer.
    fn unplan(&self, bytes: u64) {
        let _ = self.progress.total.fetch_update(Relaxed, Relaxed, |t| Some(t.saturating_sub(bytes)));
    }

    /// Ruta para abrir el archivo (std añade `\\?\` si pasa de 260 caracteres).
    #[cfg(windows)]
    fn path(&self, n: NodeId) -> std::path::PathBuf {
        self.tree.path(n, self.root_path).into()
    }

    /// Fuera de Windows `Tree::path` une con `\`: se monta con el separador del sistema.
    #[cfg(not(windows))]
    fn path(&self, n: NodeId) -> std::path::PathBuf {
        let t = self.tree;
        let mut p = std::path::PathBuf::from(self.root_path);
        for a in t.ancestors(n).into_iter().skip_while(|&a| a != t.root).skip(1) {
            p.push(t.name(a));
        }
        p
    }

    /// Huella de los primeros y últimos 64 KB (o del archivo entero si es pequeño).
    fn partial(&self, n: NodeId, size: u64) -> Option<(u128, Ident)> {
        if self.progress.cancelled() {
            return None;
        }
        let (f, ident) = open_checked(&self.path(n), size, false).ok()?;
        with_buf(|buf| {
            let h = if size <= 2 * PART {
                let b = &mut buf[..size as usize];
                read_at(&f, b, 0).ok()?;
                xxh3_128(b)
            } else {
                let mut h = Xxh3::new();
                let b = &mut buf[..PART as usize];
                read_at(&f, b, 0).ok()?;
                h.update(b);
                read_at(&f, b, size - PART).ok()?;
                h.update(b);
                h.digest128()
            };
            self.progress.done.fetch_add(size.min(2 * PART), Relaxed);
            Some((h, ident))
        })
    }

    /// Huella completa, leyendo de principio a fin en bloques de 1 MB.
    fn full(&self, n: NodeId, size: u64) -> Option<u128> {
        let p = self.progress;
        let fail = |read: u64| {
            self.unplan(size - read);
            None
        };
        if p.cancelled() {
            return fail(0);
        }
        let Ok((mut f, _)) = open_checked(&self.path(n), size, true) else { return fail(0) };
        with_buf(|buf| {
            let mut h = Xxh3::new();
            let mut read = 0u64;
            loop {
                if p.cancelled() {
                    return fail(read);
                }
                match f.read(buf) {
                    Ok(0) => break,
                    Ok(k) => {
                        h.update(&buf[..k]);
                        read += k as u64;
                        p.done.fetch_add(k as u64, Relaxed);
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => return fail(read),
                }
            }
            // si cambió de tamaño mientras se leía, no vale
            if read != size {
                return fail(read.min(size));
            }
            Some(h.digest128())
        })
    }
}

thread_local! {
    static BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Búfer de 1 MB por hilo.
fn with_buf<R>(f: impl FnOnce(&mut [u8]) -> R) -> R {
    BUF.with(|cell| match cell.try_borrow_mut() {
        Ok(mut b) => {
            if b.len() < BLOCK {
                b.resize(BLOCK, 0);
            }
            f(&mut b)
        }
        Err(_) => f(&mut vec![0u8; BLOCK]),
    })
}

fn changed() -> io::Error {
    io::Error::other("el archivo cambió o no tiene los datos en local")
}

/// Abre sin bloquear a otros programas y comprueba que sigue siendo el archivo esperado.
fn open_checked(path: &std::path::Path, size: u64, sequential: bool) -> io::Result<(File, Ident)> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_ALL: u32 = 1 | 2 | 4; // lectura, escritura y borrado
        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
        o.share_mode(FILE_SHARE_ALL);
        if sequential {
            o.custom_flags(FILE_FLAG_SEQUENTIAL_SCAN);
        }
    }
    #[cfg(not(windows))]
    let _ = sequential;
    let f = o.open(path)?;
    let (len, ident) = inspect(&f)?;
    if len != size {
        return Err(changed());
    }
    Ok((f, ident))
}

#[cfg(windows)]
fn inspect(f: &File) -> io::Result<(u64, Ident)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    // directorio, desconectado, o marcador de la nube: no se lee (no se descarga)
    const SKIP: u32 = 0x10 | 0x1000 | 0x4_0000 | 0x40_0000;
    let mut i = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: handle válido del File; estructura propia
    unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut i) }.map_err(io::Error::from)?;
    if i.dwFileAttributes & SKIP != 0 {
        return Err(changed());
    }
    let len = ((i.nFileSizeHigh as u64) << 32) | i.nFileSizeLow as u64;
    let ident = (i.nNumberOfLinks > 1).then_some((i.dwVolumeSerialNumber as u64, ((i.nFileIndexHigh as u64) << 32) | i.nFileIndexLow as u64));
    Ok((len, ident))
}

#[cfg(unix)]
fn inspect(f: &File) -> io::Result<(u64, Ident)> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(changed());
    }
    Ok((m.len(), (m.nlink() > 1).then_some((m.dev(), m.ino()))))
}

#[cfg(not(any(windows, unix)))]
fn inspect(f: &File) -> io::Result<(u64, Ident)> {
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(changed());
    }
    Ok((m.len(), None))
}

/// Lee `buf` entero desde la posición `off`.
fn read_at(f: &File, mut buf: &mut [u8], mut off: u64) -> io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let k = std::os::windows::fs::FileExt::seek_read(f, buf, off)?;
        #[cfg(unix)]
        let k = std::os::unix::fs::FileExt::read_at(f, buf, off)?;
        #[cfg(not(any(windows, unix)))]
        let k = {
            use std::io::{Seek, SeekFrom};
            let mut g = f;
            g.seek(SeekFrom::Start(off))?;
            g.read(buf)?
        };
        if k == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "archivo más corto de lo esperado"));
        }
        off += k as u64;
        buf = &mut buf[k..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Builder;

    #[test]
    fn groups_by_size_skip_flags_and_keys() {
        let mut b = Builder::default();
        b.add(1, 1, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "a", 5000, 8192, 0, 0);
        b.add(3, 1, "b", 5000, 8192, 0, 0);
        b.add(2, 1, "a-enlace", 5000, 8192, 0, 0); // enlace duro de "a"
        b.add(4, 1, "nube", 5000, 0, 0, flags::OFFLINE);
        b.add(5, 1, "union", 5000, 0, 0, flags::REPARSE);
        b.add(6, 1, "c", 9000, 12288, 0, 0);
        b.add(7, 1, "d", 9000, 12288, 0, 0);
        b.add(8, 1, "e", 9000, 12288, 0, 0);
        b.add(9, 1, "chico", 10, 0, 0, 0);
        b.add(10, 1, "chico2", 10, 0, 0, 0);
        let t = b.build(1, "R");
        let g = size_groups(&t, t.root, 100, &Progress::default());
        // primero el de mayor desperdicio posible: 3 × 9000
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].0, g[0].1.len()), (9000, 3));
        assert_eq!((g[1].0, g[1].1.len()), (5000, 2));
        let names: Vec<&str> = g[1].1.iter().map(|&n| t.name(n)).collect();
        assert!(names.contains(&"a") && names.contains(&"b"));
    }

    #[test]
    fn planned() {
        assert_eq!(planned_bytes(100), 100);
        assert_eq!(planned_bytes(2 * PART), 2 * PART);
        assert_eq!(planned_bytes(1 << 20), 2 * PART + (1 << 20));
    }

    #[test]
    fn hash_is_json_hex() {
        let g = DupGroup { size: 1, hash: 0xabc, nodes: vec![1, 2], wasted: 1 };
        let j = serde_json::to_string(&g).unwrap();
        assert!(j.contains("\"hash\":\"00000000000000000000000000000abc\""), "{j}");
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i * 31 % 251) as u8).collect();
        let mut h = Xxh3::new();
        for c in data.chunks(7000) {
            h.update(c);
        }
        assert_eq!(h.digest128(), xxh3_128(&data));
    }
}
