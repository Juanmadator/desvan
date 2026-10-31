//! Escaneo de un volumen leyendo su MFT de principio a fin, en paralelo.
//!
//! 1. El sector de arranque dice dónde está la MFT y cuánto mide cada registro.
//! 2. El registro 0 (la propia `$MFT`) dice en qué fragmentos está repartida y,
//!    con su `$BITMAP`, qué registros están en uso: así no se leen zonas vacías.
//! 3. Se reparten bloques de registros contiguos entre hilos; cada hilo lee su bloque,
//!    aplica los fixups y vuelca nombres y tamaños en su propio `Builder`.
//! 4. Se fusionan los resultados, se completan los archivos cuyos datos viven en
//!    registros de extensión y se construye el árbol.

use super::boot::Boot;
use super::record::{self, NameRef, RecordInfo};
use super::runs::{self, Run};
use super::source::Source;
use crate::tree::{flags, Builder};
use crate::util::filetime_to_unix;
use crate::{ms, Error, Method, Phase, Progress, Result, Scan, ScanStats, VolumeInfo};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Registros por bloque de lectura (4096 × 1 KB = 4 MB).
const CHUNK_RECORDS: u64 = 4096;
/// Registro del directorio raíz.
pub const ROOT_RECORD: u64 = 5;
/// Registro de `$Extend`.
const EXTEND_RECORD: u64 = 11;

#[derive(Clone, Copy, Debug)]
struct Job {
    /// Primer registro del bloque.
    first: u64,
    count: u64,
    /// Posición en el volumen.
    offset: u64,
}

/// Datos que aporta un registro de extensión a su registro base.
#[derive(Clone, Debug, Default)]
struct Patch {
    base: u64,
    data: Option<(u64, u64)>,
    named_alloc: u64,
    named_streams: u32,
    index_alloc: u64,
    /// (padre, desplazamiento, longitud) en el almacén de nombres del builder
    names: Vec<(u64, u32, u16)>,
}

/// Registro base con `$ATTRIBUTE_LIST`: puede necesitar datos de sus extensiones.
#[derive(Clone, Copy, Debug)]
struct BaseMeta {
    key: u64,
    flags: u16,
    mtime: i64,
    is_dir: bool,
}

#[derive(Default)]
struct ChunkOut {
    builder: Builder,
    patches: Vec<Patch>,
    metas: Vec<BaseMeta>,
    records: u64,
    bad: u64,
    files: u64,
    dirs: u64,
}

fn std_flags(info: &RecordInfo) -> u16 {
    let a = info.std_attrs;
    let mut f = 0;
    if a & record::FA_HIDDEN != 0 {
        f |= flags::HIDDEN;
    }
    if a & record::FA_SYSTEM != 0 {
        f |= flags::SYSTEM;
    }
    if a & record::FA_REPARSE != 0 {
        f |= flags::REPARSE;
    }
    if a & record::FA_COMPRESSED != 0 {
        f |= flags::COMPRESSED;
    }
    if a & record::FA_SPARSE != 0 {
        f |= flags::SPARSE;
    }
    if a & (record::FA_OFFLINE | record::FA_RECALL_ON_OPEN | record::FA_RECALL_ON_DATA_ACCESS) != 0 {
        f |= flags::OFFLINE;
    }
    f
}

/// Copia un nombre UTF-16 del registro al almacén del builder, en UTF-8.
fn push_name(b: &mut Builder, rec: &[u8], n: &NameRef) -> (u32, u16) {
    let off = b.names.len() as u32;
    let units = (0..n.chars).map(|i| record::u16le(rec, n.off + i * 2));
    let mut tmp = [0u8; 4];
    for ch in char::decode_utf16(units) {
        let c = ch.unwrap_or('\u{FFFD}');
        b.names.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
    }
    let len = (b.names.len() as u32 - off).min(u16::MAX as u32) as u16;
    (off, len)
}

fn process_record(rec_no: u64, rec: &mut [u8], cluster: u64, out: &mut ChunkOut) {
    if !record::apply_fixups(rec) {
        // registro vacío (todo ceros) o dañado
        if rec[0..4] != [0, 0, 0, 0] {
            out.bad += 1;
        }
        return;
    }
    let info = record::parse(rec, cluster);
    if !info.in_use {
        return;
    }
    out.records += 1;
    let mtime = filetime_to_unix(info.mtime);
    if info.base != 0 && info.base != rec_no {
        // registro de extensión: sus atributos son de otro
        let mut p = Patch {
            base: info.base,
            data: info.data,
            named_alloc: info.named_alloc,
            named_streams: info.named_streams,
            index_alloc: info.index_alloc,
            names: Vec::new(),
        };
        for n in info.names() {
            let (o, l) = push_name(&mut out.builder, rec, n);
            p.names.push((n.parent, o, l));
        }
        out.patches.push(p);
        return;
    }
    let mut f = std_flags(&info);
    if info.is_dir {
        f |= flags::DIR;
    }
    if info.named_streams > 0 && !info.is_dir {
        f |= flags::ADS;
    }
    let is_meta = (rec_no < 24 && rec_no != ROOT_RECORD) || info.names().any(|n| n.parent == EXTEND_RECORD);
    if is_meta {
        f |= flags::METAFILE;
    }
    let (size, alloc) = if info.is_dir {
        (0, info.index_alloc + info.named_alloc)
    } else {
        let (s, a) = info.data.unwrap_or((0, 0));
        (s, a + info.named_alloc)
    };
    if info.has_attr_list {
        out.metas.push(BaseMeta { key: rec_no, flags: f, mtime, is_dir: info.is_dir });
    }
    let mut any = false;
    for n in info.names() {
        let (name_off, name_len) = push_name(&mut out.builder, rec, n);
        out.builder.entries.push(crate::tree::RawEntry { key: rec_no, parent: n.parent, name_off, name_len, size, alloc, mtime, flags: f });
        any = true;
    }
    if any {
        if info.is_dir {
            out.dirs += 1;
        } else {
            out.files += 1;
        }
    }
}

/// Byte del volumen donde empieza el VCN dado de la MFT, si está asignado.
fn vcn_to_offset(runs: &[Run], vcn: u64, cluster: u64) -> Option<u64> {
    let r = runs.iter().find(|r| vcn >= r.vcn && vcn < r.vcn + r.clusters)?;
    Some((r.lcn? + (vcn - r.vcn)) * cluster)
}

/// Lee un atributo no residente completo (para el $BITMAP de la MFT o una lista de atributos).
fn read_nonresident(src: &dyn Source, runs: &[Run], size: u64, cluster: u64) -> Result<Vec<u8>> {
    let mut out = vec![0u8; size as usize];
    let mut done = 0u64;
    for r in runs {
        if done >= size {
            break;
        }
        let bytes = (r.clusters * cluster).min(size - done);
        if let Some(lcn) = r.lcn {
            src.read_at(lcn * cluster, &mut out[done as usize..(done + bytes) as usize])?;
        }
        done += bytes;
    }
    Ok(out)
}

struct MftLayout {
    runs: Vec<Run>,
    total_records: u64,
    bitmap: Option<Vec<u8>>,
}

fn attr_runs(rec: &[u8], at: usize) -> Result<(Vec<Run>, u64, u64)> {
    let a = &rec[at..];
    let start_vcn = record::u64le(a, 0x10);
    let runlist_off = record::u16le(a, 0x20) as usize;
    let len = record::u32le(a, 4) as usize;
    let runs = runs::decode(&a[runlist_off..len], start_vcn)?;
    Ok((runs, record::u64le(a, 0x30), start_vcn))
}

/// Lee el registro 0 y reconstruye dónde está toda la MFT.
fn mft_layout(src: &dyn Source, boot: &Boot) -> Result<MftLayout> {
    let rs = boot.record_size as usize;
    let mut rec0 = vec![0u8; rs];
    src.read_at(boot.mft_lcn * boot.cluster, &mut rec0)?;
    if !record::apply_fixups(&mut rec0) {
        return Err(Error::Corrupt("el registro 0 de la MFT está dañado".into()));
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut data_size = 0u64;
    for at in record::find_attr(&rec0, record::ATTR_DATA, true) {
        if rec0[at + 8] == 0 {
            continue;
        }
        let (r, size, start_vcn) = attr_runs(&rec0, at)?;
        if start_vcn == 0 {
            data_size = size;
        }
        runs.extend(r);
    }
    if runs.is_empty() {
        return Err(Error::Corrupt("la $MFT no tiene datos".into()));
    }

    // MFT muy fragmentada: el resto de sus fragmentos están en registros de extensión
    if let Some(&al) = record::find_attr(&rec0, record::ATTR_ATTRIBUTE_LIST, false).first() {
        let a = &rec0[al..];
        let list = if a[8] == 0 {
            let vo = record::u16le(a, 0x14) as usize;
            let vl = record::u32le(a, 0x10) as usize;
            a[vo..vo + vl].to_vec()
        } else {
            let (r, size, _) = attr_runs(&rec0, al)?;
            read_nonresident(src, &r, size, boot.cluster)?
        };
        let mut o = 0;
        let mut ext_records = Vec::new();
        while o + 0x1A <= list.len() {
            let ty = record::u32le(&list, o);
            let len = record::u16le(&list, o + 4) as usize;
            if len == 0 {
                break;
            }
            let name_len = list[o + 6];
            let start_vcn = record::u64le(&list, o + 8);
            let rec_no = record::mft_ref(record::u64le(&list, o + 0x10));
            if ty == record::ATTR_DATA && name_len == 0 && start_vcn > 0 && rec_no != 0 {
                ext_records.push(rec_no);
            }
            o += len;
        }
        ext_records.sort_unstable();
        ext_records.dedup();
        for rec_no in ext_records {
            let vcn = rec_no * boot.record_size as u64 / boot.cluster;
            let off = vcn_to_offset(&runs, vcn, boot.cluster).ok_or_else(|| Error::Corrupt("extensión de la $MFT inalcanzable".into()))?
                + (rec_no * boot.record_size as u64) % boot.cluster;
            let mut rec = vec![0u8; rs];
            src.read_at(off, &mut rec)?;
            if !record::apply_fixups(&mut rec) {
                continue;
            }
            for at in record::find_attr(&rec, record::ATTR_DATA, true) {
                if rec[at + 8] != 0 {
                    let (r, _, _) = attr_runs(&rec, at)?;
                    runs.extend(r);
                }
            }
        }
        runs.sort_by_key(|r| r.vcn);
    }

    let total_records = data_size / boot.record_size as u64;

    // $BITMAP de la MFT: qué registros están en uso
    let bitmap = match record::find_attr(&rec0, record::ATTR_BITMAP, true).first() {
        Some(&at) => {
            let a = &rec0[at..];
            if a[8] == 0 {
                let vo = record::u16le(a, 0x14) as usize;
                let vl = record::u32le(a, 0x10) as usize;
                Some(a[vo..vo + vl].to_vec())
            } else {
                let (r, size, _) = attr_runs(&rec0, at)?;
                read_nonresident(src, &r, size, boot.cluster).ok()
            }
        }
        None => None,
    };
    Ok(MftLayout { runs, total_records, bitmap })
}

#[inline]
fn bit(bitmap: &[u8], i: u64) -> bool {
    let byte = (i >> 3) as usize;
    byte < bitmap.len() && bitmap[byte] & (1 << (i & 7)) != 0
}

/// Trocea la MFT en bloques contiguos en disco, saltando las zonas sin registros en uso.
fn plan_jobs(layout: &MftLayout, boot: &Boot) -> Vec<Job> {
    let rs = boot.record_size as u64;
    let per_cluster = (boot.cluster / rs).max(1);
    let mut jobs = Vec::new();
    for r in &layout.runs {
        let Some(lcn) = r.lcn else { continue };
        let first_rec = r.vcn * boot.cluster / rs;
        let recs = r.clusters * per_cluster;
        let end = (first_rec + recs).min(layout.total_records);
        let mut i = first_rec;
        while i < end {
            // salta registros libres según el bitmap
            if let Some(bm) = &layout.bitmap {
                if !bit(bm, i) {
                    // avanza hasta el siguiente en uso (por bytes enteros cuando se puede)
                    while i < end && !bit(bm, i) {
                        if i & 7 == 0 && ((i >> 3) as usize) < bm.len() && bm[(i >> 3) as usize] == 0 && i + 8 <= end {
                            i += 8;
                        } else {
                            i += 1;
                        }
                    }
                    continue;
                }
            }
            let mut count = (end - i).min(CHUNK_RECORDS);
            // recorta la cola libre del bloque
            if let Some(bm) = &layout.bitmap {
                while count > 1 && !bit(bm, i + count - 1) {
                    count -= 1;
                }
            }
            let offset = lcn * boot.cluster + (i - first_rec) * rs;
            jobs.push(Job { first: i, count, offset });
            i += count;
        }
    }
    jobs
}

/// Escanea un volumen NTFS a partir de su origen de bytes.
pub fn scan(src: &dyn Source, root_path: &str, progress: &Progress) -> Result<Scan> {
    let t0 = Instant::now();
    progress.set_phase(Phase::Starting);
    let mut head = vec![0u8; 4096];
    src.read_at(0, &mut head[..512])?;
    // algunos volúmenes con sectores de 4 KB exigen leer el sector completo
    let _ = src.read_at(0, &mut head);
    let boot = Boot::parse(&head)?;
    if boot.cluster < boot.record_size as u64 && boot.record_size as u64 % boot.cluster != 0 {
        return Err(Error::Corrupt("geometría no soportada".into()));
    }
    let layout = mft_layout(src, &boot)?;
    let jobs = plan_jobs(&layout, &boot);
    let in_use: u64 = match &layout.bitmap {
        Some(bm) => (0..layout.total_records).filter(|&i| bit(bm, i)).count() as u64,
        None => layout.total_records,
    };
    progress.total.store(in_use.max(1), Ordering::Relaxed);
    progress.set_phase(Phase::Reading);

    let rs = boot.record_size as usize;
    let threads = rayon::current_num_threads();
    let t_read = Instant::now();
    let results: Vec<Result<ChunkOut>> = jobs
        .par_iter()
        .map(|job| -> Result<ChunkOut> {
            if progress.cancelled() {
                return Err(Error::Cancelled);
            }
            let mut buf = vec![0u8; job.count as usize * rs];
            src.read_at(job.offset, &mut buf)?;
            progress.bytes.fetch_add(buf.len() as u64, Ordering::Relaxed);
            let mut out = ChunkOut { builder: Builder::with_capacity(job.count as usize), ..Default::default() };
            for (k, rec) in buf.chunks_exact_mut(rs).enumerate() {
                let rec_no = job.first + k as u64;
                if let Some(bm) = &layout.bitmap {
                    if !bit(bm, rec_no) {
                        continue;
                    }
                }
                process_record(rec_no, rec, boot.cluster, &mut out);
            }
            progress.done.fetch_add(out.records, Ordering::Relaxed);
            progress.files.fetch_add(out.files, Ordering::Relaxed);
            Ok(out)
        })
        .collect();
    let read_ms = ms(t_read.elapsed());
    if progress.cancelled() {
        return Err(Error::Cancelled);
    }

    progress.set_phase(Phase::Building);
    let t_build = Instant::now();
    let mut stats = ScanStats { threads, read_ms, bytes_read: jobs.iter().map(|j| j.count * rs as u64).sum(), ..Default::default() };
    let total_entries: usize = results.iter().map(|r| r.as_ref().map(|o| o.builder.entries.len()).unwrap_or(0)).sum();
    let mut builder = Builder::with_capacity(total_entries + 16);
    let mut patches: Vec<Patch> = Vec::new();
    let mut metas: Vec<BaseMeta> = Vec::new();
    for r in results {
        let out = r?;
        stats.records += out.records;
        stats.bad_records += out.bad;
        stats.files += out.files;
        stats.dirs += out.dirs;
        let base = builder.names.len() as u32;
        for mut p in out.patches {
            for n in &mut p.names {
                n.1 += base;
            }
            patches.push(p);
        }
        metas.extend(out.metas);
        builder.merge(out.builder);
    }
    apply_patches(&mut builder, patches, metas);

    let tree = builder.build(ROOT_RECORD, root_path);
    stats.build_ms = ms(t_build.elapsed());
    stats.elapsed_ms = ms(t0.elapsed());
    progress.set_phase(Phase::Done);
    Ok(Scan {
        tree,
        root_path: root_path.to_string(),
        method: Method::Mft,
        stats,
        volume: Some(VolumeInfo { total: boot.volume_bytes(), free: 0, cluster: boot.cluster as u32, filesystem: "NTFS".into(), label: String::new() }),
    })
}

/// Completa los registros base con los datos y nombres que viven en sus extensiones.
fn apply_patches(b: &mut Builder, patches: Vec<Patch>, metas: Vec<BaseMeta>) {
    if patches.is_empty() {
        return;
    }
    #[derive(Default)]
    struct Acc {
        data: Option<(u64, u64)>,
        extra_alloc: u64,
        streams: u32,
        names: Vec<(u64, u32, u16)>,
    }
    let mut acc: HashMap<u64, Acc> = HashMap::new();
    for p in patches {
        let a = acc.entry(p.base).or_default();
        if p.data.is_some() {
            a.data = p.data;
        }
        a.extra_alloc += p.named_alloc + p.index_alloc;
        a.streams += p.named_streams;
        a.names.extend(p.names);
    }
    // plantilla (flags, fecha, tamaños) de cada base que tiene entradas
    let mut template: HashMap<u64, crate::tree::RawEntry> = HashMap::new();
    for e in b.entries.iter_mut() {
        if let Some(a) = acc.get(&e.key) {
            let is_dir = e.flags & flags::DIR != 0;
            if let Some((s, al)) = a.data {
                if !is_dir {
                    e.size = s;
                    e.alloc += al;
                }
            }
            e.alloc += a.extra_alloc;
            if a.streams > 0 && !is_dir {
                e.flags |= flags::ADS;
            }
            template.entry(e.key).or_insert_with(|| e.clone());
        }
    }
    for m in metas {
        if template.contains_key(&m.key) {
            continue;
        }
        if let Some(a) = acc.get(&m.key) {
            let (size, alloc) = if m.is_dir { (0, a.extra_alloc) } else { a.data.map(|(s, al)| (s, al + a.extra_alloc)).unwrap_or((0, a.extra_alloc)) };
            template.insert(m.key, crate::tree::RawEntry { key: m.key, parent: 0, name_off: 0, name_len: 0, size, alloc, mtime: m.mtime, flags: m.flags });
        }
    }
    // los nombres que estaban en extensiones generan sus propias entradas
    for (key, a) in acc.iter() {
        if a.names.is_empty() {
            continue;
        }
        let Some(t) = template.get(key) else { continue };
        for &(parent, off, len) in &a.names {
            let mut e = t.clone();
            e.parent = parent;
            e.name_off = off;
            e.name_len = len;
            b.entries.push(e);
        }
    }
}
