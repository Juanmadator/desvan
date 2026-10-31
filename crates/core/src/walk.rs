//! Recorrido de carpetas en paralelo: el método que no necesita permisos de administrador.
//!
//! Vale para cualquier sistema de archivos: NTFS sin elevar, exFAT/FAT32 (memorias USB)
//! y recursos de red. Cada carpeta es una tarea de rayon que lista su contenido y lanza
//! una tarea por subcarpeta (robo de trabajo); cada hilo vuelca en su propio `Builder`
//! y al final se fusionan y se construye el árbol, igual que con la MFT.
//!
//! En Windows hay dos formas de listar una carpeta:
//! - NTFS/ReFS locales: se abre la carpeta y se pide `FileIdExtdDirectoryInfo`. Es la
//!   misma consulta al núcleo que hace `FindFirstFileExW`, pero trae además el id de
//!   archivo (enlaces duros), el tamaño asignado real (comprimidos, dispersos y archivos
//!   residentes en la MFT) y, con la carpeta abierta, el tamaño de su índice.
//! - Resto (exFAT, FAT32, red…) o si lo anterior no está soportado:
//!   `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)`. El tamaño en disco
//!   se redondea al clúster y los comprimidos/dispersos se piden con `GetCompressedFileSizeW`.
//!
//! Nunca se siguen enlaces (uniones, enlaces simbólicos): quedan como nodos `REPARSE`
//! sin hijos. Los marcadores de la nube (OneDrive…) cuentan 0 en disco y no se abren.

use crate::tree::{flags, Builder, RawEntry};
use crate::{ms, Error, Method, Phase, Progress, Result, Scan, ScanStats, VolumeInfo};
use rayon::prelude::*;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Instant;

// Atributos de archivo de Windows (el modo portable los imita).
const FA_HIDDEN: u32 = 0x2;
const FA_SYSTEM: u32 = 0x4;
const FA_DIRECTORY: u32 = 0x10;
const FA_SPARSE: u32 = 0x200;
const FA_REPARSE: u32 = 0x400;
const FA_COMPRESSED: u32 = 0x800;
const FA_OFFLINE: u32 = 0x1000;
const FA_RECALL_ON_OPEN: u32 = 0x40000;
const FA_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
/// Marcadores de la nube y archivos sin datos locales: no ocupan disco y no se abren.
const FA_CLOUD: u32 = FA_OFFLINE | FA_RECALL_ON_OPEN | FA_RECALL_ON_DATA_ACCESS;

/// Clave de la carpeta raíz (las demás carpetas se numeran desde 1).
const ROOT_KEY: u64 = 0;

/// Ajustes del recorrido.
#[derive(Clone, Debug, Default)]
pub struct WalkOptions {
    /// Hilos de trabajo (0 = automático, ver [`default_threads`]).
    pub threads: usize,
    /// Usa siempre `FindFirstFileExW` aunque el volumen admita ids de archivo
    /// (diagnóstico y pruebas; solo Windows).
    pub force_find_first: bool,
}

/// Hilos por defecto: los núcleos lógicos, con un máximo de 16. Más hilos apenas
/// aceleran (el cuello está en el sistema de archivos) y castigan al resto del equipo.
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 16)
}

/// Escanea `root` (una unidad como `C:\`, una carpeta o un recurso de red) recorriendo carpetas.
pub fn scan(root: &Path, progress: &Progress) -> Result<Scan> {
    scan_with(root, progress, &WalkOptions::default())
}

/// Como [`scan`], con ajustes.
pub fn scan_with(root: &Path, progress: &Progress, opts: &WalkOptions) -> Result<Scan> {
    #[cfg(windows)]
    {
        win::scan(root, progress, opts)
    }
    #[cfg(not(windows))]
    {
        portable::scan(root, progress, opts)
    }
}

// ---------------------------------------------------------------------------
// Parte común: tareas, hilos y montaje del árbol.
// ---------------------------------------------------------------------------

/// Estado de cada hilo del recorrido.
#[derive(Default)]
struct Worker {
    b: Builder,
    files: u64,
    dirs: u64,
    denied: u64,
    bytes: u64,
    /// Ruta de trabajo en UTF-16 con nulo final.
    #[cfg(windows)]
    path: Vec<u16>,
    /// Búfer de listado (alineado a 8).
    #[cfg(windows)]
    buf: Vec<u64>,
}

/// Carpeta pendiente de listar. Su propia entrada la añade quien la lista.
struct DirJob<P> {
    path: P,
    key: u64,
    parent: u64,
    mtime: i64,
    flags: u16,
    /// Tamaño en disco ya conocido de la carpeta (modo portable).
    alloc: u64,
}

/// Resultado de listar una carpeta.
enum Listed {
    /// Listada; lleva el tamaño en disco de la propia carpeta (su índice), si se sabe.
    Ok(u64),
    /// No se pudo abrir (permisos u otro error).
    Denied,
    /// Desapareció durante el escaneo.
    Gone,
}

/// Forma de listar una carpeta (Win32 o `std::fs`).
trait Lister: Sync {
    type P: Send + 'static;
    /// Añade los archivos de la carpeta al builder del hilo y deja sus subcarpetas en `subdirs`.
    fn list(&self, ctx: &Ctx, w: &mut Worker, job: &DirJob<Self::P>, subdirs: &mut Vec<DirJob<Self::P>>) -> Listed;
    /// Guarda el nombre visible de la carpeta en el almacén de nombres.
    fn push_name(&self, names: &mut Vec<u8>, job: &DirJob<Self::P>) -> (u32, u16);
}

/// Estado compartido por todas las tareas.
struct Ctx<'a> {
    progress: &'a Progress,
    next_key: AtomicU64,
    workers: Vec<Mutex<Worker>>,
}

impl Ctx<'_> {
    fn new_key(&self) -> u64 {
        self.next_key.fetch_add(1, Relaxed)
    }
}

/// Lo que se sabe de una entrada antes de decidir qué hacer con ella.
#[cfg_attr(not(windows), allow(dead_code))]
struct Item<'n, N: ?Sized> {
    name: &'n N,
    attrs: u32,
    tag: u32,
    size: u64,
    mtime: i64,
}

/// Una carpeta: la lista, añade su entrada y lanza una tarea por subcarpeta.
fn visit<'s, 'p: 's, L: Lister>(s: &rayon::Scope<'s>, lister: &'s L, ctx: &'s Ctx<'p>, job: DirJob<L::P>) {
    if ctx.progress.cancelled() {
        return;
    }
    let mut subdirs = Vec::new();
    {
        // cada hilo usa solo su ranura: el candado nunca espera
        let slot = rayon::current_thread_index().unwrap_or(0) % ctx.workers.len();
        let mut guard = ctx.workers[slot].lock().unwrap_or_else(|e| e.into_inner());
        let w = &mut *guard;
        let (files0, bytes0) = (w.files, w.bytes);
        let (alloc, extra) = match lister.list(ctx, w, &job, &mut subdirs) {
            Listed::Ok(a) => (a + job.alloc, 0),
            Listed::Denied => {
                w.denied += 1;
                (job.alloc, flags::DENIED)
            }
            Listed::Gone => (job.alloc, 0),
        };
        let (name_off, name_len) = lister.push_name(&mut w.b.names, &job);
        w.b.entries.push(RawEntry { key: job.key, parent: job.parent, name_off, name_len, size: 0, alloc, mtime: job.mtime, flags: job.flags | flags::DIR | extra });
        w.dirs += 1;
        w.bytes += alloc;
        let p = ctx.progress;
        p.files.fetch_add(w.files - files0, Relaxed);
        p.bytes.fetch_add(w.bytes - bytes0, Relaxed);
    }
    ctx.progress.total.fetch_add(subdirs.len() as u64, Relaxed);
    ctx.progress.done.fetch_add(1, Relaxed);
    for sd in subdirs {
        s.spawn(move |s| visit(s, lister, ctx, sd));
    }
}

/// Grupo de hilos propio, con prioridad algo más baja para no entorpecer al resto del equipo.
fn build_pool(threads: usize) -> Result<rayon::ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("desvan-walk-{i}"))
        .start_handler(|_| lower_priority())
        .build()
        .map_err(|e| Error::Io(io::Error::other(e)))
}

#[cfg(windows)]
fn lower_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    // SAFETY: pseudo-handle del hilo actual
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(windows))]
fn lower_priority() {}

/// Recorre desde `root`, fusiona los builders y construye el árbol.
/// `use_ids`: las claves provisionales de los archivos son ids de archivo fiables (enlaces duros).
fn run<L: Lister>(pool: &rayon::ThreadPool, lister: &L, progress: &Progress, root: DirJob<L::P>, use_ids: bool, root_name: &str, stats: &mut ScanStats) -> Result<crate::Tree> {
    let threads = pool.current_num_threads();
    let ctx = Ctx { progress, next_key: AtomicU64::new(ROOT_KEY + 1), workers: (0..threads).map(|_| Mutex::new(Worker::default())).collect() };
    for c in [&progress.done, &progress.files, &progress.bytes] {
        c.store(0, Relaxed);
    }
    progress.total.store(1, Relaxed);
    progress.set_phase(Phase::Reading);
    let t_walk = Instant::now();
    pool.scope(|s| s.spawn(|s| visit(s, lister, &ctx, root)));
    stats.read_ms = ms(t_walk.elapsed());
    if progress.cancelled() {
        return Err(Error::Cancelled);
    }

    progress.set_phase(Phase::Building);
    let t_build = Instant::now();
    let first_file_key = ctx.next_key.load(Relaxed);
    let mut workers: Vec<Worker> = ctx.workers.into_iter().map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner())).collect();
    for w in &workers {
        stats.files += w.files;
        stats.dirs += w.dirs;
        stats.denied += w.denied;
    }
    // si ni siquiera la raíz se pudo listar, es un error, no un árbol vacío
    let root_denied = stats.denied > 0 && workers.iter().flat_map(|w| &w.b.entries).any(|e| e.key == ROOT_KEY && e.flags & flags::DIR != 0 && e.flags & flags::DENIED != 0);
    if root_denied {
        return Err(Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, "no se puede leer la carpeta (acceso denegado)")));
    }
    // se fusiona sobre el builder más grande para copiar lo mínimo
    workers.sort_by_key(|w| std::cmp::Reverse(w.b.entries.len()));
    let total: usize = workers.iter().map(|w| w.b.entries.len()).sum();
    let mut it = workers.into_iter();
    let mut b = it.next().map(|w| w.b).unwrap_or_default();
    b.entries.reserve_exact(total - b.entries.len());
    for w in it {
        b.merge(w.b);
    }
    let tree = pool.install(|| {
        assign_file_keys(&mut b.entries, first_file_key, use_ids);
        b.build(ROOT_KEY, root_name)
    });
    stats.build_ms = ms(t_build.elapsed());
    stats.threads = threads;
    Ok(tree)
}

/// Da a los archivos claves densas a continuación de las de carpetas. Con `use_ids`,
/// la clave provisional es el id de archivo: los que lo comparten son enlaces duros
/// del mismo archivo y reciben la misma clave (el árbol marca los secundarios).
fn assign_file_keys(entries: &mut [RawEntry], mut next: u64, use_ids: bool) {
    let is_file = |e: &RawEntry| e.flags & flags::DIR == 0;
    if !use_ids {
        for e in entries.iter_mut().filter(|e| is_file(e)) {
            e.key = next;
            next += 1;
        }
        return;
    }
    let mut ids: Vec<(u64, u32)> = entries.par_iter().enumerate().filter(|(_, e)| is_file(e)).map(|(i, e)| (e.key, i as u32)).collect();
    ids.par_sort_unstable();
    for run in ids.chunk_by(|a, b| a.0 == b.0) {
        if run[0].0 == 0 {
            // sin id: cada uno el suyo
            for &(_, i) in run {
                entries[i as usize].key = next;
                next += 1;
            }
        } else {
            for &(_, i) in run {
                entries[i as usize].key = next;
            }
            next += 1;
        }
    }
}

fn attr_flags(a: u32) -> u16 {
    let mut f = 0;
    if a & FA_HIDDEN != 0 {
        f |= flags::HIDDEN;
    }
    if a & FA_SYSTEM != 0 {
        f |= flags::SYSTEM;
    }
    if a & FA_COMPRESSED != 0 {
        f |= flags::COMPRESSED;
    }
    if a & FA_SPARSE != 0 {
        f |= flags::SPARSE;
    }
    if a & FA_CLOUD != 0 {
        f |= flags::OFFLINE;
    }
    f
}

/// ¿El punto de reparse es un enlace (unión, enlace simbólico, alias de aplicación)?
/// Se mira el bit "name surrogate" de la etiqueta; sin etiqueta conocida se asume
/// enlace para no arriesgarse a seguir un ciclo. Los de la nube, WOF o dedup no lo son.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_link(attrs: u32, tag: u32) -> bool {
    const NAME_SURROGATE: u32 = 0x2000_0000;
    const APPEXECLINK: u32 = 0x8000_001B;
    attrs & FA_REPARSE != 0 && (tag == 0 || tag & NAME_SURROGATE != 0 || tag == APPEXECLINK)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn round_up(n: u64, cluster: u64) -> u64 {
    n.div_ceil(cluster) * cluster
}

/// Copia un nombre UTF-16 al almacén en UTF-8 (los pares sueltos se sustituyen por U+FFFD).
#[cfg_attr(not(windows), allow(dead_code))]
fn push_utf16(names: &mut Vec<u8>, s: &[u16]) -> (u32, u16) {
    let off = names.len() as u32;
    if s.iter().all(|&c| c < 0x80) {
        names.extend(s.iter().map(|&c| c as u8));
    } else {
        let mut tmp = [0u8; 4];
        for ch in char::decode_utf16(s.iter().copied()) {
            names.extend_from_slice(ch.unwrap_or('\u{FFFD}').encode_utf8(&mut tmp).as_bytes());
        }
    }
    (off, (names.len() as u32 - off).min(u16::MAX as u32) as u16)
}

#[cfg_attr(windows, allow(dead_code))]
fn push_str(names: &mut Vec<u8>, s: &str) -> (u32, u16) {
    let off = names.len() as u32;
    let len = s.len().min(u16::MAX as usize);
    names.extend_from_slice(&s.as_bytes()[..len]);
    (off, len as u16)
}

fn unix_secs(t: std::time::SystemTime) -> i64 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

fn not_a_dir() -> Error {
    Error::Io(io::Error::new(io::ErrorKind::InvalidInput, "la ruta no es una carpeta"))
}

// ---------------------------------------------------------------------------
// Windows: Win32 directo, rutas extendidas `\\?\`.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::*;
    use crate::util::filetime_to_unix;
    use std::ffi::c_void;
    use std::mem::{offset_of, size_of};
    use std::sync::atomic::AtomicBool;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileIdExtdDirectoryInfo, FileStandardInfo, FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW,
        GetFileInformationByHandleEx, GetVolumeInformationW, GetVolumePathNameW, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_EXTD_DIR_INFO, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO,
        FIND_FIRST_EX_LARGE_FETCH, OPEN_EXISTING, WIN32_FIND_DATAW,
    };

    const ERROR_INVALID_FUNCTION: u32 = 1;
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    const ERROR_PATH_NOT_FOUND: u32 = 3;
    const ERROR_NO_MORE_FILES: u32 = 18;
    const ERROR_NOT_SUPPORTED: u32 = 50;
    const ERROR_INVALID_PARAMETER: u32 = 87;
    const ERROR_INVALID_LEVEL: u32 = 124;
    const ERROR_DIRECTORY: u32 = 267;
    const DRIVE_REMOTE: u32 = 4;
    const FILE_SUPPORTS_HARD_LINKS: u32 = 0x0040_0000;
    const BACKSLASH: u16 = b'\\' as u16;
    /// Búfer de listado: 64 KB, como `FIND_FIRST_EX_LARGE_FETCH`.
    const BUF_BYTES: usize = 64 * 1024;

    /// Ruta extendida (`\\?\…`, sin nulo) y dónde empieza el último componente.
    pub(super) struct WPath {
        buf: Vec<u16>,
        name_at: usize,
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: handle propio y válido
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    struct FindHandle(HANDLE);
    impl Drop for FindHandle {
        fn drop(&mut self) {
            // SAFETY: handle de búsqueda propio y válido
            unsafe {
                let _ = FindClose(self.0);
            }
        }
    }

    /// Código Win32 de un error de la API.
    fn code(e: &windows::core::Error) -> u32 {
        let h = e.code().0 as u32;
        if h & 0xFFFF_0000 == 0x8007_0000 {
            h & 0xFFFF
        } else {
            h
        }
    }

    fn open_error(c: u32) -> Listed {
        match c {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_DIRECTORY => Listed::Gone,
            _ => Listed::Denied,
        }
    }

    /// `dir` (+ `\` + `name`) con nulo final.
    fn set_cstr(out: &mut Vec<u16>, dir: &[u16], name: Option<&[u16]>) {
        out.clear();
        out.extend_from_slice(dir);
        if let Some(n) = name {
            if out.last() != Some(&BACKSLASH) {
                out.push(BACKSLASH);
            }
            out.extend_from_slice(n);
        }
        out.push(0);
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(b: &[u16]) -> String {
        String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
    }

    /// Id de 128 bits plegado a 64 (en NTFS la parte alta es cero).
    fn fold_id(id: &[u8; 16]) -> u64 {
        let lo = u64::from_le_bytes(id[..8].try_into().unwrap_or_default());
        let hi = u64::from_le_bytes(id[8..].try_into().unwrap_or_default());
        if hi == 0 {
            lo
        } else {
            xxhash_rust::xxh3::xxh3_64(id) | 1
        }
    }

    /// Familia FAT: sus carpetas ocupan clústeres propios que el listado no muestra.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FatKind {
        None,
        Fat,
        Exfat,
    }

    /// ¿Cabe en una entrada 8.3 sin nombre largo? (aproximado: ASCII, 8+3, un punto)
    fn is_short_name(name: &[u16]) -> bool {
        let ok = |c: &u16| *c < 0x80 && ((*c as u8).is_ascii_alphanumeric() || b"$%'-_@~`!(){}^#&".contains(&(*c as u8)));
        let dots = name.iter().filter(|&&c| c == b'.' as u16).count();
        match name.iter().position(|&c| c == b'.' as u16) {
            None => (1..=8).contains(&name.len()) && name.iter().all(ok),
            Some(i) => dots == 1 && (1..=8).contains(&i) && (1..=3).contains(&(name.len() - i - 1)) && name[..i].iter().all(ok) && name[i + 1..].iter().all(ok),
        }
    }

    struct WinLister {
        cluster: u64,
        /// Regla de residentes de NTFS (asignación no múltiplo del clúster = dentro de la MFT).
        ntfs: bool,
        fat: FatKind,
        /// Los ids de archivo identifican enlaces duros.
        use_ids: bool,
        /// Modo `FindFirstFileExW` (por volumen o porque la consulta por handle falló).
        find_first: AtomicBool,
    }

    impl Lister for WinLister {
        type P = WPath;

        fn list(&self, ctx: &Ctx, w: &mut Worker, job: &DirJob<WPath>, subdirs: &mut Vec<DirJob<WPath>>) -> Listed {
            if !self.find_first.load(Relaxed) {
                if w.buf.is_empty() {
                    w.buf = vec![0u64; BUF_BYTES / 8];
                }
                let mut buf = std::mem::take(&mut w.buf);
                let r = self.list_handle(ctx, w, &mut buf, job, subdirs);
                w.buf = buf;
                match r {
                    Some(l) => return l,
                    None => self.find_first.store(true, Relaxed),
                }
            }
            self.list_find(ctx, w, job, subdirs)
        }

        fn push_name(&self, names: &mut Vec<u8>, job: &DirJob<WPath>) -> (u32, u16) {
            push_utf16(names, &job.path.buf[job.path.name_at..])
        }
    }

    impl WinLister {
        /// Lista con la carpeta abierta. `None` si el sistema de archivos no admite la consulta.
        fn list_handle(&self, ctx: &Ctx, w: &mut Worker, buf: &mut [u64], job: &DirJob<WPath>, subdirs: &mut Vec<DirJob<WPath>>) -> Option<Listed> {
            set_cstr(&mut w.path, &job.path.buf, None);
            // SAFETY: ruta con nulo final; handle cerrado por `Handle`
            let h = match unsafe { CreateFileW(PCWSTR(w.path.as_ptr()), FILE_LIST_DIRECTORY.0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, None, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None) } {
                Ok(h) => Handle(h),
                Err(e) => return Some(open_error(code(&e))),
            };
            // tamaño del índice de la propia carpeta
            let mut alloc = 0;
            let mut si = FILE_STANDARD_INFO::default();
            // SAFETY: estructura del tamaño indicado
            if unsafe { GetFileInformationByHandleEx(h.0, FileStandardInfo, &mut si as *mut _ as *mut c_void, size_of::<FILE_STANDARD_INFO>() as u32) }.is_ok() {
                alloc = self.resident_fix(si.AllocationSize.max(0) as u64);
            }
            let bytes = (buf.len() * 8) as u32;
            let mut first = true;
            while !ctx.progress.cancelled() {
                // SAFETY: búfer propio de `bytes` bytes, alineado a 8
                if let Err(e) = unsafe { GetFileInformationByHandleEx(h.0, FileIdExtdDirectoryInfo, buf.as_mut_ptr() as *mut c_void, bytes) } {
                    let c = code(&e);
                    if c == ERROR_NO_MORE_FILES {
                        break;
                    }
                    if first && matches!(c, ERROR_INVALID_PARAMETER | ERROR_INVALID_LEVEL | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION) {
                        return None;
                    }
                    if first {
                        return Some(Listed::Denied);
                    }
                    break; // listado parcial: se queda lo que hay
                }
                first = false;
                let base = buf.as_ptr() as *const u8;
                let mut off = 0usize;
                loop {
                    // SAFETY: el sistema encadena entradas completas y alineadas dentro del búfer;
                    // el nombre (FileNameLength bytes) va justo después de la cabecera.
                    let (e, name) = unsafe {
                        let p = base.add(off);
                        let e = &*(p as *const FILE_ID_EXTD_DIR_INFO);
                        let name = std::slice::from_raw_parts(p.add(offset_of!(FILE_ID_EXTD_DIR_INFO, FileName)) as *const u16, e.FileNameLength as usize / 2);
                        (e, name)
                    };
                    let item = Item { name, attrs: e.FileAttributes, tag: e.ReparsePointTag, size: e.EndOfFile.max(0) as u64, mtime: filetime_to_unix(e.LastWriteTime as u64) };
                    let id = if self.use_ids { fold_id(&e.FileId.Identifier) } else { 0 };
                    self.add(ctx, w, job, &item, Some(e.AllocationSize.max(0) as u64), id, subdirs);
                    if e.NextEntryOffset == 0 {
                        break;
                    }
                    off += e.NextEntryOffset as usize;
                }
            }
            Some(Listed::Ok(alloc))
        }

        /// Lista con `FindFirstFileExW` (exFAT, FAT32, red…).
        fn list_find(&self, ctx: &Ctx, w: &mut Worker, job: &DirJob<WPath>, subdirs: &mut Vec<DirJob<WPath>>) -> Listed {
            set_cstr(&mut w.path, &job.path.buf, Some(&[b'*' as u16]));
            let mut fd = WIN32_FIND_DATAW::default();
            // SAFETY: patrón con nulo final; `fd` es WIN32_FIND_DATAW como pide FindExInfoBasic
            let is_root = job.parent == u64::MAX;
            let h = match unsafe { FindFirstFileExW(PCWSTR(w.path.as_ptr()), FindExInfoBasic, &mut fd as *mut _ as *mut c_void, FindExSearchNameMatch, None, FIND_FIRST_EX_LARGE_FETCH) } {
                Ok(h) => FindHandle(h),
                // una raíz vacía no devuelve ni "." ni ".."
                Err(e) if code(&e) == ERROR_FILE_NOT_FOUND => return Listed::Ok(self.fat_dir_alloc(0, is_root)),
                Err(e) => return open_error(code(&e)),
            };
            // bytes de entradas de directorio (FAT/exFAT): la carpeta ocupa sus propios clústeres
            let mut dir_bytes = 0u64;
            loop {
                let len = fd.cFileName.iter().position(|&c| c == 0).unwrap_or(fd.cFileName.len());
                dir_bytes += self.fat_entry_bytes(&fd.cFileName[..len]);
                let attrs = fd.dwFileAttributes;
                let item = Item {
                    name: &fd.cFileName[..len],
                    attrs,
                    tag: if attrs & FA_REPARSE != 0 { fd.dwReserved0 } else { 0 },
                    size: ((fd.nFileSizeHigh as u64) << 32) | fd.nFileSizeLow as u64,
                    mtime: filetime_to_unix(((fd.ftLastWriteTime.dwHighDateTime as u64) << 32) | fd.ftLastWriteTime.dwLowDateTime as u64),
                };
                self.add(ctx, w, job, &item, None, 0, subdirs);
                // SAFETY: handle de búsqueda válido
                if ctx.progress.cancelled() || unsafe { FindNextFileW(h.0, &mut fd) }.is_err() {
                    break;
                }
            }
            Listed::Ok(self.fat_dir_alloc(dir_bytes, is_root))
        }

        /// Bytes que ocupa la entrada de un nombre en una carpeta FAT (8.3 + nombre largo)
        /// o exFAT (archivo + extensión de flujo + 15 caracteres por entrada de nombre).
        fn fat_entry_bytes(&self, name: &[u16]) -> u64 {
            let dots = name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16];
            let n = name.len() as u64;
            match self.fat {
                FatKind::None => 0,
                FatKind::Exfat if dots => 0, // exFAT no los guarda: los inventa el controlador
                FatKind::Exfat => 32 * (2 + n.div_ceil(15)),
                FatKind::Fat if dots || is_short_name(name) => 32,
                FatKind::Fat => 32 * (1 + n.div_ceil(13)),
            }
        }

        /// Tamaño estimado en disco de una carpeta FAT/exFAT: sus entradas redondeadas al
        /// clúster, y al menos un clúster (con clústeres de 512 KB, miles de carpetas suman GB).
        /// Es un mínimo: las carpetas no encogen al borrar archivos.
        fn fat_dir_alloc(&self, bytes: u64, root: bool) -> u64 {
            match self.fat {
                FatKind::None => 0,
                // la raíz de exFAT guarda además el mapa de bits, la tabla de mayúsculas y la etiqueta
                _ => round_up((bytes + if root { 96 } else { 0 }).max(1), self.cluster),
            }
        }

        /// Archivos residentes en la MFT: no ocupan clústeres (como en el escáner de la MFT).
        fn resident_fix(&self, a: u64) -> u64 {
            if self.ntfs && !a.is_multiple_of(self.cluster) {
                0
            } else {
                a
            }
        }

        /// Una entrada del listado. `alloc`: asignación que da el sistema (None con FindFirstFileExW).
        #[allow(clippy::too_many_arguments)]
        fn add(&self, ctx: &Ctx, w: &mut Worker, job: &DirJob<WPath>, it: &Item<[u16]>, alloc: Option<u64>, id: u64, subdirs: &mut Vec<DirJob<WPath>>) {
            let name = it.name;
            if name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16] {
                return;
            }
            let link = is_link(it.attrs, it.tag);
            let cloud = it.attrs & FA_CLOUD != 0;
            let mut f = attr_flags(it.attrs);
            if link {
                f |= flags::REPARSE;
            }
            if it.attrs & FA_DIRECTORY != 0 {
                let key = ctx.new_key();
                if link || cloud {
                    // no se baja: ni enlaces ni carpetas de la nube sin descargar
                    let (name_off, name_len) = push_utf16(&mut w.b.names, name);
                    w.b.entries.push(RawEntry { key, parent: job.key, name_off, name_len, size: 0, alloc: 0, mtime: it.mtime, flags: f | flags::DIR });
                    w.dirs += 1;
                } else {
                    let mut buf = Vec::with_capacity(job.path.buf.len() + 1 + name.len());
                    buf.extend_from_slice(&job.path.buf);
                    if buf.last() != Some(&BACKSLASH) {
                        buf.push(BACKSLASH);
                    }
                    let name_at = buf.len();
                    buf.extend_from_slice(name);
                    subdirs.push(DirJob { path: WPath { buf, name_at }, key, parent: job.key, mtime: it.mtime, flags: f, alloc: 0 });
                }
                return;
            }
            let on_disk = if link || cloud { 0 } else { self.file_alloc(w, job, it, alloc) };
            let (name_off, name_len) = push_utf16(&mut w.b.names, name);
            w.b.entries.push(RawEntry { key: id, parent: job.key, name_off, name_len, size: it.size, alloc: on_disk, mtime: it.mtime, flags: f });
            w.files += 1;
            w.bytes += on_disk;
        }

        fn file_alloc(&self, w: &mut Worker, job: &DirJob<WPath>, it: &Item<[u16]>, alloc: Option<u64>) -> u64 {
            let rounded = round_up(it.size, self.cluster);
            match alloc {
                // con datos pero sin asignación: comprimido con WOF (CompactOS), dedup…
                Some(0) if it.size > 0 => self.compressed(w, job, it.name).map(|c| round_up(c, self.cluster)).unwrap_or(rounded),
                Some(a) => self.resident_fix(a),
                None if it.attrs & (FA_COMPRESSED | FA_SPARSE) != 0 => self.compressed(w, job, it.name).unwrap_or(rounded),
                None => rounded,
            }
        }

        /// Tamaño real en disco de un archivo comprimido, disperso o WOF.
        fn compressed(&self, w: &mut Worker, job: &DirJob<WPath>, name: &[u16]) -> Option<u64> {
            set_cstr(&mut w.path, &job.path.buf, Some(name));
            let mut hi = 0u32;
            // SAFETY: ruta con nulo final
            let lo = unsafe { GetCompressedFileSizeW(PCWSTR(w.path.as_ptr()), Some(&mut hi)) };
            // SAFETY: lectura del último error del hilo
            if lo == u32::MAX && unsafe { GetLastError() }.0 != 0 {
                return None;
            }
            Some(((hi as u64) << 32) | lo as u64)
        }
    }

    /// Datos del volumen que contiene la ruta.
    struct Vol {
        root: String,
        info: VolumeInfo,
        remote: bool,
        hardlinks: bool,
    }

    /// Raíz del volumen deducida del texto (`C:\`, `\\servidor\recurso\`).
    fn textual_volume_root(disp: &str) -> String {
        if let Some(rest) = disp.strip_prefix(r"\\") {
            let mut parts = rest.splitn(3, '\\');
            let server = parts.next().unwrap_or("");
            let share = parts.next().unwrap_or("");
            format!(r"\\{server}\{share}\")
        } else {
            format!("{}\\", &disp[..disp.len().min(2)])
        }
    }

    fn volume(disp: &str) -> Vol {
        let mut vbuf = vec![0u16; 1024];
        // SAFETY: ruta con nulo final y búfer propio
        let root = if unsafe { GetVolumePathNameW(PCWSTR(wide(disp).as_ptr()), &mut vbuf) }.is_ok() { from_wide(&vbuf) } else { textual_volume_root(disp) };
        let wroot = wide(&root);
        let r = PCWSTR(wroot.as_ptr());
        let (mut label, mut fsname) = ([0u16; 261], [0u16; 261]);
        let mut fsflags = 0u32;
        // SAFETY: búferes propios del tamaño indicado
        let _ = unsafe { GetVolumeInformationW(r, Some(&mut label), None, None, Some(&mut fsflags), Some(&mut fsname)) };
        let (mut spc, mut bps) = (0u32, 0u32);
        // SAFETY: salidas propias
        let cluster = if unsafe { GetDiskFreeSpaceW(r, Some(&mut spc), Some(&mut bps), None, None) }.is_ok() && spc * bps > 0 { spc * bps } else { 4096 };
        let (mut total, mut free) = (0u64, 0u64);
        // SAFETY: salidas propias
        let _ = unsafe { GetDiskFreeSpaceExW(r, None, Some(&mut total), Some(&mut free)) };
        // SAFETY: ruta con nulo final
        let remote = unsafe { GetDriveTypeW(r) } == DRIVE_REMOTE;
        Vol { root, info: VolumeInfo { total, free, cluster, filesystem: from_wide(&fsname), label: from_wide(&label) }, remote, hardlinks: fsflags & FILE_SUPPORTS_HARD_LINKS != 0 }
    }

    /// Ruta visible (sin `\\?\`, sin barra final salvo en la raíz de unidad).
    fn display_path(root: &Path) -> io::Result<String> {
        let s = root.as_os_str().to_string_lossy().replace('/', "\\");
        // "C:" a secas es la carpeta actual de C:; aquí se entiende la unidad
        let abs = if s.len() == 2 && s.ends_with(':') { format!("{s}\\") } else { std::path::absolute(root)?.to_string_lossy().into_owned() };
        let mut d = if let Some(rest) = abs.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{rest}")
        } else if let Some(rest) = abs.strip_prefix(r"\\?\") {
            if rest.as_bytes().get(1) == Some(&b':') {
                rest.to_string()
            } else {
                abs.clone() // \\?\Volume{…}\ y similares: se dejan tal cual
            }
        } else {
            abs.clone()
        };
        while d.ends_with('\\') && !(d.len() == 3 && d.as_bytes()[1] == b':') && !d.starts_with(r"\\?\") {
            d.pop();
        }
        Ok(d)
    }

    fn extended(disp: &str) -> Vec<u16> {
        let s = if disp.starts_with(r"\\?\") {
            disp.to_string()
        } else if let Some(rest) = disp.strip_prefix(r"\\") {
            format!(r"\\?\UNC\{rest}")
        } else {
            format!(r"\\?\{disp}")
        };
        s.encode_utf16().collect()
    }

    fn same_path(a: &str, b: &str) -> bool {
        a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
    }

    pub(super) fn scan(root: &Path, progress: &Progress, opts: &WalkOptions) -> Result<Scan> {
        let t0 = Instant::now();
        progress.set_phase(Phase::Starting);
        let disp = display_path(root)?;
        let vol = volume(&disp);
        let is_root = same_path(&disp, &vol.root);
        let root_name = if is_root { vol.root.clone() } else { disp.clone() };
        let mut ext = extended(&root_name);
        if is_root && ext.last() != Some(&BACKSLASH) {
            ext.push(BACKSLASH); // `\\?\C:` sin barra sería el volumen, no su carpeta raíz
        }
        let md = std::fs::metadata(String::from_utf16_lossy(&ext))?;
        if !md.is_dir() {
            return Err(not_a_dir());
        }
        let fs = vol.info.filesystem.to_ascii_uppercase();
        let by_handle = !opts.force_find_first && !vol.remote && (fs == "NTFS" || fs == "REFS");
        let fat = match fs.as_str() {
            "EXFAT" => FatKind::Exfat,
            "FAT" | "FAT12" | "FAT16" | "FAT32" => FatKind::Fat,
            _ => FatKind::None,
        };
        let lister = WinLister { cluster: vol.info.cluster.max(512) as u64, ntfs: fs == "NTFS", fat, use_ids: by_handle && vol.hardlinks, find_first: AtomicBool::new(!by_handle) };
        let root_job = DirJob { path: WPath { buf: ext, name_at: 0 }, key: ROOT_KEY, parent: u64::MAX, mtime: md.modified().map(unix_secs).unwrap_or(0), flags: 0, alloc: 0 };
        let threads = if opts.threads == 0 { default_threads() } else { opts.threads };
        let pool = build_pool(threads)?;
        let mut stats = ScanStats::default();
        let tree = run(&pool, &lister, progress, root_job, lister.use_ids, &root_name, &mut stats)?;
        stats.elapsed_ms = ms(t0.elapsed());
        progress.set_phase(Phase::Done);
        Ok(Scan { tree, root_path: root_name, method: Method::Walk, stats, volume: is_root.then_some(vol.info) })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn paths() {
            assert_eq!(display_path(Path::new("C:")).unwrap(), r"C:\");
            assert_eq!(display_path(Path::new(r"C:\")).unwrap(), r"C:\");
            assert_eq!(display_path(Path::new(r"C:\Users\")).unwrap(), r"C:\Users");
            assert_eq!(display_path(Path::new(r"\\?\C:\Users")).unwrap(), r"C:\Users");
            assert_eq!(display_path(Path::new(r"\\?\UNC\srv\rec\x")).unwrap(), r"\\srv\rec\x");
            assert_eq!(String::from_utf16_lossy(&extended(r"C:\Users")), r"\\?\C:\Users");
            assert_eq!(String::from_utf16_lossy(&extended(r"\\srv\rec")), r"\\?\UNC\srv\rec");
            assert_eq!(textual_volume_root(r"\\srv\rec\a\b"), r"\\srv\rec\");
            assert_eq!(textual_volume_root(r"D:\a"), r"D:\");
            assert!(same_path(r"C:\", r"c:"));
        }

        #[test]
        fn fat_directory_sizes() {
            let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
            assert!(is_short_name(&w("README.TXT")) && is_short_name(&w("A")) && is_short_name(&w("FOUND.000")));
            assert!(!is_short_name(&w("nombre largo.txt")) && !is_short_name(&w("a.b.c")) && !is_short_name(&w("ñ.txt")));
            let l = |fat| WinLister { cluster: 512 * 1024, ntfs: false, fat, use_ids: false, find_first: AtomicBool::new(true) };
            let ex = l(FatKind::Exfat);
            assert_eq!(ex.fat_entry_bytes(&w("..")), 0);
            assert_eq!(ex.fat_entry_bytes(&w("foto.jpg")), 96); // 2 + 1 entradas de nombre
            assert_eq!(ex.fat_entry_bytes(&w("un nombre de más de 15")), 128);
            assert_eq!(ex.fat_dir_alloc(0, false), 512 * 1024, "al menos un clúster");
            assert_eq!(ex.fat_dir_alloc(600_000, false), 1024 * 1024);
            let fat = l(FatKind::Fat);
            assert_eq!(fat.fat_entry_bytes(&w(".")), 32);
            assert_eq!(fat.fat_entry_bytes(&w("DATOS.BIN")), 32);
            assert_eq!(fat.fat_entry_bytes(&w("datos largos.bin")), 32 * 3);
            assert_eq!(l(FatKind::None).fat_dir_alloc(5000, true), 0);
        }

        #[test]
        fn ids() {
            let mut id = [0u8; 16];
            id[0] = 7;
            assert_eq!(fold_id(&id), 7);
            id[9] = 1;
            assert_ne!(fold_id(&id), 7);
        }
    }
}

// ---------------------------------------------------------------------------
// Portable: `std::fs` (Linux, macOS…). En Windows se compila solo en pruebas.
// ---------------------------------------------------------------------------

#[cfg(any(not(windows), test))]
mod portable {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    pub(super) struct StdLister {
        cluster: u64,
        /// Dispositivo de la raíz: no se cruza a otros sistemas de archivos montados.
        dev: u64,
    }

    #[cfg(unix)]
    fn dev_of(md: &fs::Metadata) -> u64 {
        use std::os::unix::fs::MetadataExt;
        md.dev()
    }
    #[cfg(not(unix))]
    fn dev_of(_: &fs::Metadata) -> u64 {
        0
    }

    /// (tamaño en disco, id para enlaces duros o 0).
    #[cfg(unix)]
    fn disk_info(md: &fs::Metadata, _cluster: u64) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt;
        let id = if !md.is_dir() && md.nlink() > 1 {
            let mut k = [0u8; 16];
            k[..8].copy_from_slice(&md.dev().to_le_bytes());
            k[8..].copy_from_slice(&md.ino().to_le_bytes());
            xxhash_rust::xxh3::xxh3_64(&k) | 1
        } else {
            0
        };
        (md.blocks() * 512, id)
    }
    #[cfg(not(unix))]
    fn disk_info(md: &fs::Metadata, cluster: u64) -> (u64, u64) {
        (if md.is_dir() { 0 } else { round_up(md.len(), cluster) }, 0)
    }

    #[cfg(windows)]
    fn attrs_of(md: &fs::Metadata, _name: &str) -> u32 {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes()
    }
    #[cfg(not(windows))]
    fn attrs_of(md: &fs::Metadata, name: &str) -> u32 {
        let mut a = if md.is_dir() { FA_DIRECTORY } else { 0 };
        if name.starts_with('.') {
            a |= FA_HIDDEN;
        }
        a
    }

    #[cfg(windows)]
    fn link_is_dir(ft: &fs::FileType) -> bool {
        use std::os::windows::fs::FileTypeExt;
        ft.is_symlink_dir()
    }
    #[cfg(not(windows))]
    fn link_is_dir(_: &fs::FileType) -> bool {
        false
    }

    impl Lister for StdLister {
        type P = PathBuf;

        fn list(&self, ctx: &Ctx, w: &mut Worker, job: &DirJob<PathBuf>, subdirs: &mut Vec<DirJob<PathBuf>>) -> Listed {
            let rd = match fs::read_dir(&job.path) {
                Ok(r) => r,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Listed::Gone,
                Err(_) => return Listed::Denied,
            };
            for de in rd {
                if ctx.progress.cancelled() {
                    break;
                }
                // DirEntry::metadata no sigue enlaces
                let Ok(de) = de else { continue };
                let Ok(md) = de.metadata() else { continue };
                let os_name = de.file_name();
                let name = os_name.to_string_lossy();
                let ft = md.file_type();
                let it = Item { name: name.as_ref(), attrs: attrs_of(&md, &name), tag: 0, size: md.len(), mtime: md.modified().map(unix_secs).unwrap_or(0) };
                let mut f = attr_flags(it.attrs);
                if ft.is_symlink() {
                    // enlace: hoja sin tamaño
                    let dir = link_is_dir(&ft);
                    let key = if dir { ctx.new_key() } else { 0 };
                    let (name_off, name_len) = push_str(&mut w.b.names, it.name);
                    w.b.entries.push(RawEntry { key, parent: job.key, name_off, name_len, size: 0, alloc: 0, mtime: it.mtime, flags: f | flags::REPARSE | if dir { flags::DIR } else { 0 } });
                    if dir {
                        w.dirs += 1;
                    } else {
                        w.files += 1;
                    }
                    continue;
                }
                let (alloc, id) = disk_info(&md, self.cluster);
                if ft.is_dir() {
                    let key = ctx.new_key();
                    if dev_of(&md) != self.dev || it.attrs & FA_CLOUD != 0 {
                        // otro sistema de archivos montado aquí, o carpeta de la nube: no se baja
                        f |= if dev_of(&md) != self.dev { flags::REPARSE } else { 0 };
                        let (name_off, name_len) = push_str(&mut w.b.names, it.name);
                        w.b.entries.push(RawEntry { key, parent: job.key, name_off, name_len, size: 0, alloc: 0, mtime: it.mtime, flags: f | flags::DIR });
                        w.dirs += 1;
                    } else {
                        subdirs.push(DirJob { path: job.path.join(&os_name), key, parent: job.key, mtime: it.mtime, flags: f, alloc });
                    }
                    continue;
                }
                let alloc = if it.attrs & FA_CLOUD != 0 { 0 } else { alloc };
                let (name_off, name_len) = push_str(&mut w.b.names, it.name);
                w.b.entries.push(RawEntry { key: id, parent: job.key, name_off, name_len, size: it.size, alloc, mtime: it.mtime, flags: f });
                w.files += 1;
                w.bytes += alloc;
            }
            Listed::Ok(0)
        }

        fn push_name(&self, names: &mut Vec<u8>, job: &DirJob<PathBuf>) -> (u32, u16) {
            let n = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            push_str(names, &n)
        }
    }

    pub(super) fn scan(root: &Path, progress: &Progress, opts: &WalkOptions) -> Result<Scan> {
        let t0 = Instant::now();
        progress.set_phase(Phase::Starting);
        let abs = std::path::absolute(root)?;
        let md = fs::metadata(&abs)?;
        if !md.is_dir() {
            return Err(not_a_dir());
        }
        let root_name = abs.to_string_lossy().into_owned();
        let lister = StdLister { cluster: 4096, dev: dev_of(&md) };
        let (alloc, _) = disk_info(&md, lister.cluster);
        let root_job = DirJob { path: abs, key: ROOT_KEY, parent: u64::MAX, mtime: md.modified().map(unix_secs).unwrap_or(0), flags: 0, alloc };
        let threads = if opts.threads == 0 { default_threads() } else { opts.threads };
        let pool = build_pool(threads)?;
        let mut stats = ScanStats::default();
        let tree = run(&pool, &lister, progress, root_job, cfg!(unix), &root_name, &mut stats)?;
        stats.elapsed_ms = ms(t0.elapsed());
        progress.set_phase(Phase::Done);
        Ok(Scan { tree, root_path: root_name, method: Method::Walk, stats, volume: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeId, Tree, NONE};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("desvan-walk-unit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn find(t: &Tree, name: &str) -> NodeId {
        (0..t.len() as NodeId).find(|&n| t.name(n) == name).unwrap_or(NONE)
    }

    #[test]
    fn file_keys_and_hard_links() {
        let mk = |key: u64, dir: bool| RawEntry { key, parent: 0, name_off: 0, name_len: 0, size: 0, alloc: 0, mtime: 0, flags: if dir { flags::DIR } else { 0 } };
        let mut v = vec![mk(5, true), mk(900, false), mk(0, false), mk(900, false), mk(0, false), mk(77, false)];
        assign_file_keys(&mut v, 10, true);
        assert_eq!(v[0].key, 5); // las carpetas no se tocan
        assert_eq!(v[1].key, v[3].key); // mismo id: mismo archivo
        let mut keys: Vec<u64> = v[1..].iter().map(|e| e.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 4);
        assert!(keys.iter().all(|&k| (10..14).contains(&k)));
        let mut v = vec![mk(900, false), mk(900, false)];
        assign_file_keys(&mut v, 3, false);
        assert_ne!(v[0].key, v[1].key);
    }

    #[test]
    fn link_tags() {
        assert!(is_link(FA_REPARSE, 0xA000_0003)); // unión
        assert!(is_link(FA_REPARSE, 0xA000_000C)); // enlace simbólico
        assert!(is_link(FA_REPARSE, 0)); // etiqueta desconocida: no se sigue
        assert!(!is_link(FA_REPARSE, 0x9000_601A)); // OneDrive
        assert!(!is_link(FA_REPARSE, 0x8000_0017)); // WOF
        assert!(!is_link(0, 0xA000_0003));
    }

    /// El modo portable (std::fs) y el nativo cuentan lo mismo.
    #[test]
    fn portable_matches_native() {
        let d = temp_dir("cmp");
        std::fs::create_dir_all(d.join("a/b/c")).unwrap();
        std::fs::create_dir_all(d.join("vacía")).unwrap();
        std::fs::write(d.join("a/uno.bin"), vec![1u8; 5000]).unwrap();
        std::fs::write(d.join("a/b/dos.txt"), b"hola").unwrap();
        std::fs::write(d.join("a/b/c/tres.dat"), vec![3u8; 70_000]).unwrap();
        let p = Progress::default();
        let native = scan(&d, &p).unwrap();
        let port = portable::scan(&d, &p, &WalkOptions { threads: 3, ..Default::default() }).unwrap();
        for s in [&native, &port] {
            let t = &s.tree;
            assert_eq!(t.files[t.root as usize], 3);
            assert_eq!(t.dirs[t.root as usize], 4);
            assert_eq!(t.size[t.root as usize], 75_004);
            assert_ne!(find(t, "tres.dat"), NONE);
            assert_ne!(find(t, "vacía"), NONE);
        }
        assert_eq!(port.stats.denied, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cancel() {
        let d = temp_dir("cancel");
        std::fs::create_dir_all(d.join("x")).unwrap();
        let p = Progress::default();
        p.cancel.store(true, Relaxed);
        assert!(matches!(scan(&d, &p), Err(Error::Cancelled)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn not_a_directory() {
        let d = temp_dir("file");
        let f = d.join("x.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(matches!(scan(&f, &Progress::default()), Err(Error::Io(_))));
        let _ = std::fs::remove_dir_all(&d);
    }
}
