//! Medición del recorrido de carpetas (solo lectura).
//!
//! `cargo run --release -p desvan-core --example walk_bench -- C:\ [--threads N] [--find-first] [--top N] [--junk] [--dupes]`
//!
//! - `--junk`: aplica las reglas de limpieza al árbol (no toca el disco).
//! - `--dupes`: busca duplicados (≥ 1 MB) leyendo los archivos candidatos.

use desvan_core::dupes::{find_dupes, DupOptions};
use desvan_core::junk::find_junk;
use desvan_core::util::human;
use desvan_core::walk::{scan_with, WalkOptions};
use desvan_core::{flags, Progress};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    let mut opts = WalkOptions::default();
    let (mut top, mut junk, mut dupes) = (12usize, false, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--threads" => opts.threads = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--find-first" => opts.force_find_first = true,
            "--top" => top = args.next().and_then(|v| v.parse().ok()).unwrap_or(12),
            "--junk" => junk = true,
            "--dupes" => dupes = true,
            _ => root = Some(PathBuf::from(a)),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let progress = Progress::default();
    let t0 = Instant::now();
    let scan = std::thread::scope(|s| {
        let h = s.spawn(|| scan_with(&root, &progress, &opts));
        // progreso cada 2 s
        let mut last = Instant::now();
        while !h.is_finished() {
            std::thread::sleep(Duration::from_millis(50));
            if last.elapsed() >= Duration::from_secs(2) {
                last = Instant::now();
                let p = progress.snapshot();
                eprintln!("  … {:>6.1} s  carpetas {}/{}  archivos {}  {}", t0.elapsed().as_secs_f64(), p.done, p.total, p.files, human(p.bytes));
            }
        }
        h.join().unwrap()
    });
    let scan = match scan {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let wall = t0.elapsed().as_secs_f64();
    let t = &scan.tree;
    let st = &scan.stats;
    let r = t.root as usize;
    println!("raíz        {}", scan.root_path);
    println!("hilos       {}  (FindFirstFileExW forzado: {})", st.threads, opts.force_find_first);
    println!("archivos    {}   carpetas {}   denegadas {}", st.files, st.dirs, st.denied);
    println!("tiempo      {wall:.2} s  (recorrido {} ms, árbol {} ms)", st.read_ms, st.build_ms);
    println!("ritmo       {:.0} archivos/s  ({:.0} entradas/s)", st.files as f64 / wall, (st.files + st.dirs) as f64 / wall);
    println!("tamaño      {}   en disco {}", human(t.size[r]), human(t.alloc[r]));
    println!("memoria     árbol ≈ {}", human(t.heap_bytes() as u64));
    if let Some(v) = &scan.volume {
        let used = v.total.saturating_sub(v.free);
        println!("volumen     {} «{}»  total {}  libre {}  usado {}  clúster {}", v.filesystem, v.label, human(v.total), human(v.free), human(used), v.cluster);
        println!("cobertura   {:.1} % del espacio usado", t.alloc[r] as f64 * 100.0 / used.max(1) as f64);
    }
    let count = |f: u16| (0..t.len()).filter(|&i| t.flags[i] & f != 0).count();
    println!("marcas      enlaces duros {}  reparse {}  nube {}  denegadas {}", count(flags::HARDLINK), count(flags::REPARSE), count(flags::OFFLINE), count(flags::DENIED));
    println!("mayores:");
    for &c in t.children(t.root).iter().take(top) {
        println!("  {:>10}  {:>10}  {}", human(t.alloc[c as usize]), human(t.size[c as usize]), t.name(c));
    }

    if junk {
        let tj = Instant::now();
        let hits = find_junk(t, &scan.root_path);
        println!("limpieza    {} sugerencias en {} ms", hits.len(), tj.elapsed().as_millis());
        for h in &hits {
            println!("  {:>10}  {:<12} {:<24} {} nodo(s)", human(h.bytes), format!("{:?}", h.risk), h.rule, h.nodes.len());
            for &n in h.nodes.iter().take(3) {
                println!("              {:>10}  {}", human(t.alloc[n as usize]), t.path(n, &scan.root_path));
            }
        }
    }

    if dupes {
        let p = Progress::default();
        let td = Instant::now();
        let groups = find_dupes(t, &scan.root_path, &DupOptions::default(), &p, |_| {}).unwrap();
        let s = p.snapshot();
        let wasted: u64 = groups.iter().map(|g| g.wasted).sum();
        let secs = td.elapsed().as_secs_f64();
        println!("duplicados  {} grupos, {} desperdiciados; leídos {} en {secs:.1} s ({}/s)", groups.len(), human(wasted), human(s.done), human((s.done as f64 / secs.max(0.001)) as u64));
        for g in groups.iter().take(5) {
            println!("  {:>10} × {}  {}", human(g.size), g.nodes.len(), t.path(g.nodes[0], &scan.root_path));
        }
    }
}
