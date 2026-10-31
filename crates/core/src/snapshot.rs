//! Serialización binaria de un escaneo (árbol + metadatos), para pasarlo del ayudante
//! elevado a la interfaz por una tubería y, más adelante, para guardarlo en caché.
//!
//! Formato (little-endian): "DSVT", u32 versión, u32 longitud del JSON de metadatos y el JSON;
//! después, cada array del árbol como u64 longitud + datos crudos.

use crate::tree::{NodeId, Tree};
use crate::{Method, Scan, ScanStats, VolumeInfo};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

const MAGIC: &[u8; 4] = b"DSVT";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Meta {
    root_path: String,
    method: String,
    stats: ScanStats,
    volume: Option<VolumeInfo>,
    root: NodeId,
    orphans: NodeId,
    exts: Vec<String>,
}

/// Datos planos que se pueden copiar como bytes.
trait Pod: Copy + Default {
    const SIZE: usize;
    fn put(self, out: &mut Vec<u8>);
    fn get(b: &[u8]) -> Self;
}

macro_rules! pod {
    ($($t:ty),*) => {$(
        impl Pod for $t {
            const SIZE: usize = std::mem::size_of::<$t>();
            #[inline]
            fn put(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            #[inline]
            fn get(b: &[u8]) -> Self {
                <$t>::from_le_bytes(b.try_into().unwrap())
            }
        }
    )*};
}
pod!(u8, u16, u32, u64, i64);

fn write_vec<T: Pod, W: Write>(w: &mut W, v: &[T]) -> io::Result<()> {
    w.write_all(&(v.len() as u64).to_le_bytes())?;
    // en bloques para no duplicar en memoria arrays de cientos de MB
    let mut buf = Vec::with_capacity((1 << 20).min(v.len() * T::SIZE + 8));
    for chunk in v.chunks((1 << 20) / T::SIZE.max(1)) {
        buf.clear();
        for &x in chunk {
            x.put(&mut buf);
        }
        w.write_all(&buf)?;
    }
    Ok(())
}

fn read_vec<T: Pod, R: Read>(r: &mut R) -> io::Result<Vec<T>> {
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let n = u64::from_le_bytes(len) as usize;
    if n > (1usize << 34) / T::SIZE.max(1) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "array demasiado grande"));
    }
    let mut out = Vec::with_capacity(n);
    let mut buf = vec![0u8; (1 << 20).max(T::SIZE)];
    let per = buf.len() / T::SIZE;
    let mut left = n;
    while left > 0 {
        let k = left.min(per);
        let bytes = &mut buf[..k * T::SIZE];
        r.read_exact(bytes)?;
        out.extend(bytes.chunks_exact(T::SIZE).map(T::get));
        left -= k;
    }
    Ok(out)
}

/// Escribe un escaneo completo.
pub fn write_scan<W: Write>(w: &mut W, s: &Scan) -> io::Result<()> {
    let t = &s.tree;
    let meta = Meta {
        root_path: s.root_path.clone(),
        method: match s.method {
            Method::Mft => "mft".into(),
            Method::Walk => "walk".into(),
        },
        stats: s.stats.clone(),
        volume: s.volume.clone(),
        root: t.root,
        orphans: t.orphans,
        exts: t.exts.clone(),
    };
    let json = serde_json::to_vec(&meta).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    w.write_all(MAGIC)?;
    w.write_all(&VERSION.to_le_bytes())?;
    w.write_all(&(json.len() as u32).to_le_bytes())?;
    w.write_all(&json)?;
    write_vec(w, &t.parent)?;
    write_vec(w, &t.child_start)?;
    write_vec(w, &t.children)?;
    write_vec(w, &t.size)?;
    write_vec(w, &t.alloc)?;
    write_vec(w, &t.files)?;
    write_vec(w, &t.dirs)?;
    write_vec(w, &t.mtime)?;
    write_vec(w, &t.flags)?;
    write_vec(w, &t.ext)?;
    write_vec(w, &t.name_off)?;
    write_vec(w, &t.name_len)?;
    write_vec(w, &t.names)?;
    write_vec(w, &t.key)?;
    w.flush()
}

/// Lee un escaneo escrito con [`write_scan`].
pub fn read_scan<R: Read>(r: &mut R) -> io::Result<Scan> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut head = [0u8; 12];
    r.read_exact(&mut head)?;
    if &head[0..4] != MAGIC {
        return Err(bad("no es un escaneo de Desván"));
    }
    if u32::from_le_bytes(head[4..8].try_into().unwrap()) != VERSION {
        return Err(bad("versión de escaneo no soportada"));
    }
    let jl = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
    let mut json = vec![0u8; jl];
    r.read_exact(&mut json)?;
    let meta: Meta = serde_json::from_slice(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tree = Tree {
        parent: read_vec(r)?,
        child_start: read_vec(r)?,
        children: read_vec(r)?,
        size: read_vec(r)?,
        alloc: read_vec(r)?,
        files: read_vec(r)?,
        dirs: read_vec(r)?,
        mtime: read_vec(r)?,
        flags: read_vec(r)?,
        ext: read_vec(r)?,
        name_off: read_vec(r)?,
        name_len: read_vec(r)?,
        names: read_vec(r)?,
        exts: meta.exts,
        key: read_vec(r)?,
        root: meta.root,
        orphans: meta.orphans,
    };
    let n = tree.parent.len();
    if tree.child_start.len() != n + 1 || tree.size.len() != n || tree.name_off.len() != n || (tree.root as usize) >= n.max(1) {
        return Err(bad("árbol incoherente"));
    }
    Ok(Scan {
        tree,
        root_path: meta.root_path,
        method: if meta.method == "mft" { Method::Mft } else { Method::Walk },
        stats: meta.stats,
        volume: meta.volume,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{flags, Builder};

    #[test]
    fn round_trip() {
        let mut b = Builder::default();
        b.add(1, 1, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "fotos", 0, 0, 0, flags::DIR);
        b.add(3, 2, "playa.JPG", 5000, 8192, 1_700_000_000, 0);
        b.add(4, 1, "ñandú.txt", 10, 0, 0, flags::HIDDEN);
        let s = Scan { tree: b.build(1, "X:\\"), root_path: "X:\\".into(), method: Method::Mft, stats: ScanStats { files: 2, ..Default::default() }, volume: None };
        let mut buf = Vec::new();
        write_scan(&mut buf, &s).unwrap();
        let back = read_scan(&mut buf.as_slice()).unwrap();
        let t = &back.tree;
        assert_eq!(back.method, Method::Mft);
        assert_eq!(back.stats.files, 2);
        assert_eq!(t.len(), s.tree.len());
        assert_eq!(t.alloc, s.tree.alloc);
        assert_eq!(t.exts, s.tree.exts);
        let fotos = t.children(t.root).iter().copied().find(|&c| t.name(c) == "fotos").unwrap();
        assert_eq!(t.path(t.children(fotos)[0], "X:\\"), "X:\\fotos\\playa.JPG");
        assert_eq!(t.ext_name(t.children(fotos)[0]), "jpg");
        // datos corruptos
        assert!(read_scan(&mut &buf[..20]).is_err());
        assert!(read_scan(&mut &b"XXXX0000"[..]).is_err());
    }
}
