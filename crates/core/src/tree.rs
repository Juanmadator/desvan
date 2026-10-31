//! Árbol compacto de archivos (estructura de arrays + hijos en formato CSR).
//!
//! Los escáneres (MFT o recorrido de carpetas) no construyen el árbol directamente:
//! van acumulando [`RawEntry`] en un [`Builder`], que al final resuelve padres,
//! huérfanos y enlaces duros, suma tamaños de abajo arriba y ordena los hijos.

use rayon::prelude::*;
use std::collections::HashMap;

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

pub mod flags {
    pub const DIR: u16 = 1 << 0;
    pub const HIDDEN: u16 = 1 << 1;
    pub const SYSTEM: u16 = 1 << 2;
    /// Enlace simbólico o unión: no se desciende por él.
    pub const REPARSE: u16 = 1 << 3;
    pub const COMPRESSED: u16 = 1 << 4;
    pub const SPARSE: u16 = 1 << 5;
    /// Enlace duro secundario: se muestra, pero no suma en los totales.
    pub const HARDLINK: u16 = 1 << 6;
    /// Tiene flujos de datos alternativos (ADS).
    pub const ADS: u16 = 1 << 7;
    pub const ORPHAN: u16 = 1 << 8;
    /// Nodo creado por Desván ("huérfanos", "espacio libre"…).
    pub const SYNTHETIC: u16 = 1 << 9;
    /// Archivo de metadatos de NTFS ($MFT, $LogFile…).
    pub const METAFILE: u16 = 1 << 10;
    /// Marcador de la nube (OneDrive…): no ocupa disco.
    pub const OFFLINE: u16 = 1 << 11;
    /// No se pudo leer (permisos).
    pub const DENIED: u16 = 1 << 12;
    /// Enviado a la Papelera desde Desván (su espacio ya se ha descontado).
    pub const DELETED: u16 = 1 << 13;
}

/// Entrada recogida por un escáner antes de existir el árbol.
#[derive(Clone, Debug)]
pub struct RawEntry {
    /// Clave única del objeto (nº de registro de la MFT o id del recorrido).
    /// Los enlaces duros comparten clave.
    pub key: u64,
    /// Clave del directorio padre.
    pub parent: u64,
    pub name_off: u32,
    pub name_len: u16,
    /// Tamaño lógico (bytes del contenido).
    pub size: u64,
    /// Tamaño en disco (clústeres asignados).
    pub alloc: u64,
    /// Fecha de modificación (segundos Unix).
    pub mtime: i64,
    pub flags: u16,
}

/// Acumula entradas y nombres. Varios builders (uno por hilo) se pueden fusionar.
#[derive(Default)]
pub struct Builder {
    pub names: Vec<u8>,
    pub entries: Vec<RawEntry>,
}

impl Builder {
    pub fn with_capacity(entries: usize) -> Self {
        Self { names: Vec::with_capacity(entries * 24), entries: Vec::with_capacity(entries) }
    }

    /// Guarda un nombre en el almacén y devuelve (desplazamiento, longitud).
    pub fn push_name(&mut self, name: &str) -> (u32, u16) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(u16::MAX as usize);
        let off = self.names.len() as u32;
        self.names.extend_from_slice(&bytes[..len]);
        (off, len as u16)
    }

    pub fn add(&mut self, key: u64, parent: u64, name: &str, size: u64, alloc: u64, mtime: i64, flags: u16) {
        let (name_off, name_len) = self.push_name(name);
        self.entries.push(RawEntry { key, parent, name_off, name_len, size, alloc, mtime, flags });
    }

    pub fn name(&self, e: &RawEntry) -> &str {
        std::str::from_utf8(&self.names[e.name_off as usize..e.name_off as usize + e.name_len as usize]).unwrap_or("?")
    }

    /// Añade al final las entradas de otro builder, corrigiendo los desplazamientos de nombre.
    pub fn merge(&mut self, mut other: Builder) {
        let base = self.names.len() as u32;
        self.names.append(&mut other.names);
        self.entries.reserve(other.entries.len());
        for mut e in other.entries {
            e.name_off += base;
            self.entries.push(e);
        }
    }

    /// Construye el árbol. `root_key` es la clave del directorio raíz y `root_name` su nombre visible.
    pub fn build(self, root_key: u64, root_name: &str) -> Tree {
        build_tree(self, root_key, root_name)
    }
}

/// Árbol inmutable y compacto. Los hijos de cada directorio están contiguos en
/// `children[child_start[n]..child_start[n+1]]`, ordenados de mayor a menor tamaño en disco.
pub struct Tree {
    pub parent: Vec<NodeId>,
    pub child_start: Vec<u32>,
    pub children: Vec<NodeId>,
    pub size: Vec<u64>,
    pub alloc: Vec<u64>,
    /// Archivos que cuelgan del nodo (1 para un archivo).
    pub files: Vec<u32>,
    /// Subcarpetas que cuelgan del nodo.
    pub dirs: Vec<u32>,
    pub mtime: Vec<i64>,
    pub flags: Vec<u16>,
    pub ext: Vec<u16>,
    pub name_off: Vec<u32>,
    pub name_len: Vec<u16>,
    pub names: Vec<u8>,
    /// Tabla de extensiones (índice 0 = sin extensión).
    pub exts: Vec<String>,
    pub key: Vec<u64>,
    pub root: NodeId,
    /// Nodo sintético que agrupa los huérfanos, si los hay.
    pub orphans: NodeId,
}

impl Tree {
    pub fn len(&self) -> usize {
        self.parent.len()
    }
    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }
    pub fn name(&self, n: NodeId) -> &str {
        let o = self.name_off[n as usize] as usize;
        let l = self.name_len[n as usize] as usize;
        std::str::from_utf8(&self.names[o..o + l]).unwrap_or("?")
    }
    pub fn children(&self, n: NodeId) -> &[NodeId] {
        let s = self.child_start[n as usize] as usize;
        let e = self.child_start[n as usize + 1] as usize;
        &self.children[s..e]
    }
    pub fn is_dir(&self, n: NodeId) -> bool {
        self.flags[n as usize] & flags::DIR != 0
    }
    pub fn ext_name(&self, n: NodeId) -> &str {
        &self.exts[self.ext[n as usize] as usize]
    }
    /// Profundidad (la raíz es 0).
    pub fn depth(&self, mut n: NodeId) -> u32 {
        let mut d = 0;
        while self.parent[n as usize] != NONE {
            n = self.parent[n as usize];
            d += 1;
        }
        d
    }
    /// Ruta completa. `root_path` es la ruta de la raíz (p. ej. `C:\`).
    pub fn path(&self, n: NodeId, root_path: &str) -> String {
        let mut parts = Vec::new();
        let mut cur = n;
        while cur != self.root && cur != NONE {
            parts.push(self.name(cur));
            cur = self.parent[cur as usize];
        }
        let mut s = root_path.to_string();
        for (i, p) in parts.iter().rev().enumerate() {
            if i > 0 || !(s.ends_with('\\') || s.ends_with('/')) {
                s.push('\\');
            }
            s.push_str(p);
        }
        s
    }
    /// Cadena de antepasados desde la raíz hasta `n` (incluidos).
    pub fn ancestors(&self, n: NodeId) -> Vec<NodeId> {
        let mut v = vec![n];
        let mut cur = n;
        while self.parent[cur as usize] != NONE {
            cur = self.parent[cur as usize];
            v.push(cur);
        }
        v.reverse();
        v
    }
    /// Memoria aproximada que ocupa el árbol.
    pub fn heap_bytes(&self) -> usize {
        let n = self.len();
        n * (4 + 4 + 4 + 8 + 8 + 4 + 4 + 8 + 2 + 2 + 4 + 2 + 8) + self.names.len() + self.exts.iter().map(|e| e.len() + 24).sum::<usize>()
    }
}

fn build_tree(b: Builder, root_key: u64, root_name: &str) -> Tree {
    let Builder { mut names, entries } = b;
    // nodos: 0 = raíz (sintética si el escáner no la aportó), luego las entradas, luego "huérfanos"
    let root_entry = entries.iter().position(|e| e.key == root_key && e.flags & flags::DIR != 0);
    let mut n_nodes = entries.len() + if root_entry.is_some() { 0 } else { 1 };

    // índice de cada entrada -> nodo
    let mut parent_key: Vec<u64> = Vec::with_capacity(n_nodes);
    let mut key: Vec<u64> = Vec::with_capacity(n_nodes);
    let mut size = Vec::with_capacity(n_nodes);
    let mut alloc = Vec::with_capacity(n_nodes);
    let mut mtime = Vec::with_capacity(n_nodes);
    let mut flg: Vec<u16> = Vec::with_capacity(n_nodes);
    let mut name_off = Vec::with_capacity(n_nodes);
    let mut name_len = Vec::with_capacity(n_nodes);

    let root_name_off = names.len() as u32;
    names.extend_from_slice(root_name.as_bytes());
    let root: NodeId;
    if let Some(ri) = root_entry {
        root = ri as NodeId;
    } else {
        root = entries.len() as NodeId;
    }
    for e in &entries {
        parent_key.push(e.parent);
        key.push(e.key);
        size.push(e.size);
        alloc.push(e.alloc);
        mtime.push(e.mtime);
        flg.push(e.flags);
        name_off.push(e.name_off);
        name_len.push(e.name_len);
    }
    drop(entries);
    if root_entry.is_none() {
        parent_key.push(u64::MAX);
        key.push(root_key);
        size.push(0);
        alloc.push(0);
        mtime.push(0);
        flg.push(flags::DIR | flags::SYNTHETIC);
        name_off.push(root_name_off);
        name_len.push(root_name.len() as u16);
    }
    name_off[root as usize] = root_name_off;
    name_len[root as usize] = root_name.len() as u16;

    // clave -> nodo, solo para directorios (son los únicos que pueden ser padres)
    let max_key = key.iter().copied().filter(|&k| k != u64::MAX).max().unwrap_or(0);
    let dense = max_key < 256 * 1024 * 1024;
    let mut dir_dense: Vec<NodeId> = if dense { vec![NONE; max_key as usize + 1] } else { Vec::new() };
    let mut dir_sparse: HashMap<u64, NodeId> = HashMap::new();
    for i in 0..n_nodes {
        if flg[i] & flags::DIR != 0 {
            let k = key[i];
            if dense {
                if dir_dense[k as usize] == NONE {
                    dir_dense[k as usize] = i as NodeId;
                }
            } else {
                dir_sparse.entry(k).or_insert(i as NodeId);
            }
        }
    }
    let lookup = |k: u64| -> NodeId {
        if dense {
            if (k as usize) < dir_dense.len() {
                dir_dense[k as usize]
            } else {
                NONE
            }
        } else {
            *dir_sparse.get(&k).unwrap_or(&NONE)
        }
    };

    // enlaces duros: la primera aparición de una clave de archivo es la principal
    {
        let mut seen: HashMap<u64, ()> = HashMap::new();
        if dense {
            let mut bits = vec![0u64; (max_key as usize >> 6) + 1];
            for i in 0..n_nodes {
                if flg[i] & flags::DIR != 0 || flg[i] & flags::SYNTHETIC != 0 {
                    continue;
                }
                let k = key[i] as usize;
                let (w, b) = (k >> 6, 1u64 << (k & 63));
                if bits[w] & b != 0 {
                    flg[i] |= flags::HARDLINK;
                } else {
                    bits[w] |= b;
                }
            }
        } else {
            for i in 0..n_nodes {
                if flg[i] & (flags::DIR | flags::SYNTHETIC) != 0 {
                    continue;
                }
                if seen.insert(key[i], ()).is_some() {
                    flg[i] |= flags::HARDLINK;
                }
            }
        }
    }

    // padres
    let mut parent: Vec<NodeId> = vec![NONE; n_nodes];
    let mut orphan_list: Vec<NodeId> = Vec::new();
    for i in 0..n_nodes {
        if i as NodeId == root {
            continue;
        }
        let p = lookup(parent_key[i]);
        if p == NONE || p == i as NodeId {
            orphan_list.push(i as NodeId);
        } else {
            parent[i] = p;
        }
    }

    // ciclos: subiendo por los padres, todo debe acabar en la raíz o en un huérfano.
    // Si una cadena vuelve sobre sí misma, se rompe el ciclo colgando ese nodo de "huérfanos".
    {
        // 0 = sin visitar, 1 = en el camino actual, 2 = resuelto
        let mut state = vec![0u8; n_nodes];
        state[root as usize] = 2;
        for &o in &orphan_list {
            state[o as usize] = 2;
        }
        let mut path = Vec::new();
        for i in 0..n_nodes {
            if state[i] != 0 {
                continue;
            }
            path.clear();
            let mut cur = i as NodeId;
            loop {
                match state[cur as usize] {
                    0 => {
                        state[cur as usize] = 1;
                        path.push(cur);
                        cur = parent[cur as usize];
                    }
                    1 => {
                        orphan_list.push(cur);
                        break;
                    }
                    _ => break,
                }
            }
            for &s in &path {
                state[s as usize] = 2;
            }
        }
    }
    let mut orphans = NONE;
    if !orphan_list.is_empty() {
        orphans = n_nodes as NodeId;
        n_nodes += 1;
        let (o, l) = {
            let off = names.len() as u32;
            names.extend_from_slice("‹huérfanos›".as_bytes());
            (off, "‹huérfanos›".len() as u16)
        };
        parent.push(root);
        key.push(u64::MAX);
        size.push(0);
        alloc.push(0);
        mtime.push(0);
        flg.push(flags::DIR | flags::SYNTHETIC);
        name_off.push(o);
        name_len.push(l);
        for &x in &orphan_list {
            parent[x as usize] = orphans;
            flg[x as usize] |= flags::ORPHAN;
        }
    }

    // hijos en CSR
    let mut counts = vec![0u32; n_nodes + 1];
    for i in 0..n_nodes {
        let p = parent[i];
        if p != NONE {
            counts[p as usize] += 1;
        }
    }
    let mut child_start = vec![0u32; n_nodes + 1];
    for i in 0..n_nodes {
        child_start[i + 1] = child_start[i] + counts[i];
    }
    let mut fill = child_start.clone();
    let mut children = vec![0 as NodeId; child_start[n_nodes] as usize];
    for i in 0..n_nodes {
        let p = parent[i];
        if p != NONE {
            children[fill[p as usize] as usize] = i as NodeId;
            fill[p as usize] += 1;
        }
    }
    drop(fill);
    drop(counts);

    // orden de recorrido desde la raíz (BFS) para sumar de abajo arriba
    let mut order: Vec<NodeId> = Vec::with_capacity(n_nodes);
    order.push(root);
    let mut head = 0;
    while head < order.len() {
        let n = order[head] as usize;
        head += 1;
        let (s, e) = (child_start[n] as usize, child_start[n + 1] as usize);
        order.extend_from_slice(&children[s..e]);
    }
    let mut files = vec![0u32; n_nodes];
    let mut dirs = vec![0u32; n_nodes];
    for &n in order.iter().rev() {
        let i = n as usize;
        let f = flg[i];
        let is_dir = f & flags::DIR != 0;
        if !is_dir {
            files[i] = 1;
        }
        let p = parent[i];
        if p == NONE {
            continue;
        }
        let pi = p as usize;
        if f & flags::HARDLINK == 0 {
            size[pi] += size[i];
            alloc[pi] += alloc[i];
        }
        files[pi] += files[i];
        dirs[pi] += dirs[i] + is_dir as u32;
    }
    drop(order);

    // hijos ordenados por tamaño en disco (y luego lógico, y nombre)
    {
        let mut slices: Vec<&mut [NodeId]> = Vec::new();
        let mut rest: &mut [NodeId] = &mut children;
        for n in 0..n_nodes {
            let len = (child_start[n + 1] - child_start[n]) as usize;
            let (a, b) = rest.split_at_mut(len);
            if len > 1 {
                slices.push(a);
            }
            rest = b;
        }
        let alloc_ref = &alloc;
        let size_ref = &size;
        slices.par_iter_mut().for_each(|s| {
            s.sort_unstable_by(|&a, &b| alloc_ref[b as usize].cmp(&alloc_ref[a as usize]).then(size_ref[b as usize].cmp(&size_ref[a as usize])).then(a.cmp(&b)));
        });
    }

    // extensiones: la extensión en minúsculas cabe en un u128 (hasta 12 bytes + longitud),
    // así se agrupan sin crear un String por archivo
    let ext_key = |i: usize| -> u128 {
        if flg[i] & flags::DIR != 0 {
            return 0;
        }
        let o = name_off[i] as usize;
        let nm = &names[o..o + name_len[i] as usize];
        let Some(dot) = nm.iter().rposition(|&b| b == b'.') else { return 0 };
        let e = &nm[dot + 1..];
        if dot == 0 || e.is_empty() || e.len() > 12 || e.contains(&b' ') || std::str::from_utf8(e).is_err() {
            return 0;
        }
        let mut k: u128 = e.len() as u128;
        for (j, &b) in e.iter().enumerate() {
            k |= (b.to_ascii_lowercase() as u128) << (8 * (j + 1));
        }
        k
    };
    let keys: Vec<u128> = (0..n_nodes).into_par_iter().map(ext_key).collect();
    let mut exts: Vec<String> = vec![String::new()];
    let mut ext_ids: HashMap<u128, u16> = HashMap::new();
    let mut ext = vec![0u16; n_nodes];
    for (i, &k) in keys.iter().enumerate() {
        if k == 0 {
            continue;
        }
        let id = match ext_ids.get(&k) {
            Some(&id) => id,
            None => {
                if exts.len() >= u16::MAX as usize {
                    0
                } else {
                    let len = (k & 0xFF) as usize;
                    let bytes: Vec<u8> = (0..len).map(|j| (k >> (8 * (j + 1))) as u8).collect();
                    let id = exts.len() as u16;
                    exts.push(String::from_utf8_lossy(&bytes).into_owned());
                    ext_ids.insert(k, id);
                    id
                }
            }
        };
        ext[i] = id;
    }
    drop(keys);

    Tree {
        parent,
        child_start,
        children,
        size,
        alloc,
        files,
        dirs,
        mtime,
        flags: flg,
        ext,
        name_off,
        name_len,
        names,
        exts,
        key,
        root,
        orphans,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tree {
        let mut b = Builder::default();
        b.add(1, 0, "raiz", 0, 0, 0, flags::DIR);
        b.add(2, 1, "docs", 0, 0, 0, flags::DIR);
        b.add(3, 2, "a.txt", 100, 4096, 0, 0);
        b.add(4, 2, "b.MP4", 5000, 8192, 0, 0);
        b.add(5, 1, "c.bin", 10, 4096, 0, 0);
        b.add(4, 1, "enlace.mp4", 5000, 8192, 0, 0); // enlace duro de b.MP4
        b.add(6, 99, "perdido.dat", 7, 4096, 0, 0); // padre inexistente
        b.build(1, "C:\\")
    }

    #[test]
    fn sums_sorts_and_links() {
        let t = sample();
        assert_eq!(t.name(t.root), "C:\\");
        assert_eq!(t.alloc[t.root as usize], 4096 + 8192 + 4096 + 4096); // el enlace duro no suma
        let kids: Vec<&str> = t.children(t.root).iter().map(|&c| t.name(c)).collect();
        assert_eq!(kids[0], "docs");
        let docs = t.children(t.root)[0];
        assert_eq!(t.files[docs as usize], 2);
        assert_eq!(t.alloc[docs as usize], 12288);
        let link = t.children(t.root).iter().copied().find(|&c| t.name(c) == "enlace.mp4").unwrap();
        assert!(t.flags[link as usize] & flags::HARDLINK != 0);
        assert_eq!(t.ext_name(docs), "");
        let mp4 = t.children(docs)[0];
        assert_eq!(t.name(mp4), "b.MP4");
        assert_eq!(t.ext_name(mp4), "mp4");
    }

    #[test]
    fn orphans_and_paths() {
        let t = sample();
        assert_ne!(t.orphans, NONE);
        let lost = t.children(t.orphans)[0];
        assert_eq!(t.name(lost), "perdido.dat");
        let docs = t.children(t.root)[0];
        let a = *t.children(docs).last().unwrap();
        assert_eq!(t.path(a, "C:\\"), "C:\\docs\\a.txt");
        assert_eq!(t.depth(a), 2);
        assert_eq!(t.ancestors(a).len(), 3);
    }

    #[test]
    fn cycles_become_orphans() {
        let mut b = Builder::default();
        b.add(1, 1, "", 0, 0, 0, flags::DIR);
        b.add(2, 3, "x", 0, 0, 0, flags::DIR);
        b.add(3, 2, "y", 0, 0, 0, flags::DIR);
        b.add(4, 3, "f", 1, 4096, 0, 0);
        let t = b.build(1, "R");
        // el ciclo x<->y acaba colgando de huérfanos y el total cuadra
        assert_ne!(t.orphans, NONE);
        assert_eq!(t.alloc[t.root as usize], 4096);
    }

    #[test]
    fn merge_builders() {
        let mut a = Builder::default();
        a.add(1, 1, "", 0, 0, 0, flags::DIR);
        let mut b = Builder::default();
        b.add(2, 1, "hola", 3, 4096, 0, 0);
        a.merge(b);
        let t = a.build(1, "X:\\");
        assert_eq!(t.name(t.children(t.root)[0]), "hola");
    }
}
