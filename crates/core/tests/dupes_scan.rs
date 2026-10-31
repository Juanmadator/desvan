//! Pruebas del buscador de duplicados sobre archivos temporales reales.

use desvan_core::dupes::{find_dupes, find_dupes_in, DupGroup, DupOptions};
use desvan_core::walk::{scan, scan_with, WalkOptions};
use desvan_core::{NodeId, Phase, Progress, Scan, Tree, NONE};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("desvan-dupes-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Contenido pseudoaleatorio reproducible.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn find(t: &Tree, name: &str) -> NodeId {
    (0..t.len() as NodeId).find(|&n| t.name(n) == name).unwrap_or(NONE)
}

fn names(t: &Tree, g: &DupGroup) -> Vec<String> {
    let mut v: Vec<String> = g.nodes.iter().map(|&n| t.name(n).to_string()).collect();
    v.sort();
    v
}

fn opts(min_size: u64) -> DupOptions {
    DupOptions { min_size, max_open: 4 }
}

fn dupes(s: &Scan, min_size: u64) -> Vec<DupGroup> {
    find_dupes(&s.tree, &s.root_path, &opts(min_size), &Progress::default(), |_| {}).unwrap()
}

/// Árbol con todos los casos.
fn sample(tag: &str) -> (TempDir, Scan) {
    let d = TempDir::new(tag);
    let r = &d.0;
    fs::create_dir_all(r.join("copias/más")).unwrap();
    // tres copias de 300 KB (parcial + completa)
    let big = noise(300_000, 1);
    fs::write(r.join("grande.bin"), &big).unwrap();
    fs::write(r.join("copias/grande (1).bin"), &big).unwrap();
    fs::write(r.join("copias/más/grande (2).bin"), &big).unwrap();
    // dos copias pequeñas (la parcial ya cubre el archivo entero)
    let small = noise(10_000, 2);
    fs::write(r.join("chico.txt"), &small).unwrap();
    fs::write(r.join("copias/chico.txt"), &small).unwrap();
    // mismo tamaño, contenido distinto
    fs::write(r.join("distinto_a.bin"), noise(50_000, 3)).unwrap();
    fs::write(r.join("distinto_b.bin"), noise(50_000, 4)).unwrap();
    // mismo principio, distinto final
    let mut tail_a = noise(400_000, 5);
    let mut tail_b = tail_a.clone();
    *tail_a.last_mut().unwrap() ^= 0xFF;
    tail_b[399_000] ^= 0x01;
    fs::write(r.join("cola_a.bin"), &tail_a).unwrap();
    fs::write(r.join("cola_b.bin"), &tail_b).unwrap();
    // mismos primeros y últimos 64 KB, distinto en medio: solo lo pilla la huella completa
    let mid_a = noise(500_000, 6);
    let mut mid_b = mid_a.clone();
    mid_b[250_000] ^= 0x55;
    fs::write(r.join("medio_a.bin"), &mid_a).unwrap();
    fs::write(r.join("medio_b.bin"), &mid_b).unwrap();
    // vacíos
    for i in 0..3 {
        fs::write(r.join(format!("vacío{i}.txt")), b"").unwrap();
    }
    let s = scan(r, &Progress::default()).unwrap();
    (d, s)
}

#[test]
fn finds_exact_copies_only() {
    let (_d, s) = sample("exactas");
    let t = &s.tree;
    let groups = dupes(&s, 1);
    assert_eq!(groups.len(), 2, "{:?}", groups.iter().map(|g| names(t, g)).collect::<Vec<_>>());
    // de mayor a menor desperdicio
    assert_eq!(names(t, &groups[0]), vec!["grande (1).bin", "grande (2).bin", "grande.bin"]);
    assert_eq!(groups[0].size, 300_000);
    assert_eq!(groups[0].wasted, 600_000);
    assert_eq!(names(t, &groups[1]), vec!["chico.txt", "chico.txt"]);
    assert_eq!(groups[1].wasted, 10_000);
    assert_eq!(groups[0].hash, xxhash_rust::xxh3::xxh3_128(&noise(300_000, 1)));
    assert_eq!(groups[1].hash, xxhash_rust::xxh3::xxh3_128(&noise(10_000, 2)));
}

#[test]
fn min_size_filters() {
    let (_d, s) = sample("minimo");
    let groups = dupes(&s, 100_000);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].size, 300_000);
    // por defecto, 1 MB: nada
    let none = find_dupes(&s.tree, &s.root_path, &DupOptions::default(), &Progress::default(), |_| {}).unwrap();
    assert!(none.is_empty());
}

#[test]
fn callback_progress_and_subtree() {
    let (_d, s) = sample("aviso");
    let t = &s.tree;
    let seen = Mutex::new(Vec::new());
    let p = Progress::default();
    let groups = find_dupes(t, &s.root_path, &opts(1), &p, |g| seen.lock().unwrap().push(g.wasted)).unwrap();
    let mut seen = seen.into_inner().unwrap();
    seen.sort_unstable();
    let mut want: Vec<u64> = groups.iter().map(|g| g.wasted).collect();
    want.sort_unstable();
    assert_eq!(seen, want, "un aviso por grupo confirmado");
    let ps = p.snapshot();
    assert_eq!(ps.phase, Phase::Done);
    assert_eq!(ps.done, ps.total, "al terminar se ha leído todo lo previsto");
    assert_eq!(ps.bytes, 610_000);
    assert_eq!(ps.files, 11, "cada candidato resuelto una vez");

    // solo dentro de «copias»: grande (1) + grande (2) y nada más
    let copias = find(t, "copias");
    let sub = find_dupes_in(t, copias, &s.root_path, &opts(1), &Progress::default(), |_| {}).unwrap();
    assert_eq!(sub.len(), 1);
    assert_eq!(names(t, &sub[0]), vec!["grande (1).bin", "grande (2).bin"]);
    // un archivo suelto como raíz: no hay con quién comparar
    let one = find_dupes_in(t, find(t, "grande.bin"), &s.root_path, &opts(1), &Progress::default(), |_| {}).unwrap();
    assert!(one.is_empty());
}

#[test]
fn hard_links_are_not_duplicates() {
    let d = TempDir::new("enlaces");
    let r = &d.0;
    let data = noise(200_000, 9);
    fs::write(r.join("a.bin"), &data).unwrap();
    fs::hard_link(r.join("a.bin"), r.join("a-enlace.bin")).unwrap();
    fs::write(r.join("copia.bin"), &data).unwrap();
    // solo enlaces, sin copia real: nunca es un grupo
    fs::write(r.join("solo.bin"), noise(150_000, 10)).unwrap();
    fs::hard_link(r.join("solo.bin"), r.join("solo-enlace.bin")).unwrap();

    let check = |s: &Scan| {
        let t = &s.tree;
        let groups = dupes(s, 1);
        assert_eq!(groups.len(), 1, "{:?}", groups.iter().map(|g| names(t, g)).collect::<Vec<_>>());
        let g = &groups[0];
        assert_eq!(g.nodes.len(), 2);
        assert!(names(t, g).contains(&"copia.bin".to_string()));
        assert_eq!(g.wasted, 200_000);
    };
    // árbol con enlaces marcados (misma clave)
    check(&scan(r, &Progress::default()).unwrap());
    // árbol sin marcar (FindFirstFileExW no da ids): se detectan al abrirlos
    #[cfg(windows)]
    check(&scan_with(r, &Progress::default(), &WalkOptions { force_find_first: true, ..Default::default() }).unwrap());
    #[cfg(not(windows))]
    let _ = (scan_with, WalkOptions::default());
}

#[test]
fn vanished_or_changed_files_are_skipped() {
    let d = TempDir::new("cambios");
    let r = &d.0;
    let data = noise(120_000, 11);
    for n in ["x.bin", "y.bin", "z.bin"] {
        fs::write(r.join(n), &data).unwrap();
    }
    let s = scan(r, &Progress::default()).unwrap();
    fs::remove_file(r.join("z.bin")).unwrap();
    let groups = dupes(&s, 1);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].nodes.len(), 2);
    fs::write(r.join("y.bin"), noise(120_001, 12)).unwrap(); // cambia de tamaño
    assert!(dupes(&s, 1).is_empty());
}

#[test]
fn cancel_stops_quickly() {
    let (_d, s) = sample("cancelar");
    let p = Progress::default();
    p.cancel.store(true, Relaxed);
    let calls = Mutex::new(0);
    let r = find_dupes(&s.tree, &s.root_path, &opts(1), &p, |_| *calls.lock().unwrap() += 1);
    assert!(matches!(r, Err(desvan_core::Error::Cancelled)));
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[test]
fn files_stay_usable_while_hashing() {
    // se abren compartiendo lectura, escritura y borrado: otro programa puede tocarlos
    let d = TempDir::new("compartir");
    let r: &Path = &d.0;
    let data = noise(3 << 20, 13);
    fs::write(r.join("p.bin"), &data).unwrap();
    fs::write(r.join("q.bin"), &data).unwrap();
    let s = scan(r, &Progress::default()).unwrap();
    let held = fs::OpenOptions::new().read(true).write(true).open(r.join("p.bin")).unwrap();
    let groups = find_dupes(&s.tree, &s.root_path, &DupOptions::default(), &Progress::default(), |_| {}).unwrap();
    drop(held);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].wasted, 3 << 20);
}
