//! `desvan`: escaneo y mediciones desde la línea de comandos.
//!
//!   desvan scan C:                 escanea un volumen (MFT si hay permisos)
//!   desvan scan --image disco.img  escanea una imagen NTFS
//!   desvan scan C: --top 20 --json resumen en JSON

use anyhow::{bail, Context, Result};
use desvan_core::ntfs::{self, FileSource};
use desvan_core::util::human;
use desvan_core::{Progress, Scan, Tree};
use std::path::PathBuf;
use std::time::Instant;

struct Args {
    target: Option<String>,
    image: Option<PathBuf>,
    top: usize,
    json: bool,
    repeat: usize,
}

fn parse_args() -> Result<(String, Args)> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_else(|| "help".into());
    let mut a = Args { target: None, image: None, top: 15, json: false, repeat: 1 };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--image" => a.image = Some(PathBuf::from(it.next().context("falta la ruta de la imagen")?)),
            "--top" => a.top = it.next().context("falta N")?.parse()?,
            "--repeat" => a.repeat = it.next().context("falta N")?.parse()?,
            "--json" => a.json = true,
            _ if a.target.is_none() => a.target = Some(x),
            _ => bail!("argumento desconocido: {x}"),
        }
    }
    Ok((cmd, a))
}

fn run_scan(a: &Args) -> Result<Scan> {
    let progress = Progress::default();
    if let Some(img) = &a.image {
        let src = FileSource::open_image(img).with_context(|| format!("no se puede abrir {}", img.display()))?;
        return Ok(ntfs::scan(&src, "IMG:\\", &progress)?);
    }
    let target = a.target.clone().context("indica una unidad (C:) o --image")?;
    let letter = target.chars().next().context("unidad vacía")?;
    #[cfg(windows)]
    {
        let src = FileSource::open_volume(letter)?;
        Ok(ntfs::scan(&src, &format!("{}:\\", letter.to_ascii_uppercase()), &progress)?)
    }
    #[cfg(not(windows))]
    {
        let _ = letter;
        bail!("la lectura de volúmenes solo está disponible en Windows")
    }
}

fn print_top(t: &Tree, n: usize) {
    let total = t.alloc[t.root as usize].max(1);
    for &c in t.children(t.root).iter().take(n) {
        let i = c as usize;
        println!(
            "  {:>10}  {:>5.1}%  {:>9} arch.  {}{}",
            human(t.alloc[i]),
            t.alloc[i] as f64 * 100.0 / total as f64,
            t.files[i],
            t.name(c),
            if t.is_dir(c) { "\\" } else { "" }
        );
    }
}

fn main() -> Result<()> {
    let (cmd, a) = parse_args()?;
    match cmd.as_str() {
        "scan" => {
            let mut last = None;
            for i in 0..a.repeat {
                let t0 = Instant::now();
                let s = run_scan(&a)?;
                if a.repeat > 1 {
                    eprintln!("pasada {}: {} ms", i + 1, t0.elapsed().as_millis());
                }
                last = Some(s);
            }
            let s = last.unwrap();
            let t = &s.tree;
            if a.json {
                let top: Vec<_> = t
                    .children(t.root)
                    .iter()
                    .take(a.top)
                    .map(|&c| serde_json::json!({"name": t.name(c), "alloc": t.alloc[c as usize], "size": t.size[c as usize], "files": t.files[c as usize]}))
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "method": s.method, "stats": s.stats, "nodes": t.len(), "tree_bytes": t.heap_bytes(),
                        "alloc": t.alloc[t.root as usize], "size": t.size[t.root as usize], "top": top
                    }))?
                );
            } else {
                let st = &s.stats;
                println!("{} — método {:?}", s.root_path, s.method);
                println!(
                    "  {} archivos, {} carpetas, {} registros ({} dañados) en {} ms  [lectura {} ms, árbol {} ms, {} hilos, {} leídos]",
                    st.files,
                    st.dirs,
                    st.records,
                    st.bad_records,
                    st.elapsed_ms,
                    st.read_ms,
                    st.build_ms,
                    st.threads,
                    human(st.bytes_read)
                );
                println!("  ocupado: {} en disco ({} de datos), árbol en memoria: {}", human(t.alloc[t.root as usize]), human(t.size[t.root as usize]), human(t.heap_bytes() as u64));
                print_top(t, a.top);
            }
        }
        #[cfg(windows)]
        "bench" => {
            // compara lectura con caché y sin caché en el mismo proceso
            let target = a.target.clone().context("indica una unidad (C:)")?;
            let letter = target.chars().next().context("unidad vacía")?;
            let root = format!("{}:\\", letter.to_ascii_uppercase());
            let mut results = Vec::new();
            let modes: Vec<bool> = match std::env::var("DESVAN_MODE").as_deref() {
                Ok("sin") => vec![true],
                Ok("con") => vec![false],
                _ => vec![true, false],
            };
            for unbuffered in modes {
                for i in 0..a.repeat {
                    let src = FileSource::open_volume_with(letter, unbuffered)?;
                    ntfs::scan::IO_NS.store(0, std::sync::atomic::Ordering::Relaxed);
                    ntfs::scan::PARSE_NS.store(0, std::sync::atomic::Ordering::Relaxed);
                    let t0 = Instant::now();
                    let s = ntfs::scan(&src, &root, &Progress::default())?;
                    let io = ntfs::scan::IO_NS.load(std::sync::atomic::Ordering::Relaxed) / 1_000_000;
                    let parse = ntfs::scan::PARSE_NS.load(std::sync::atomic::Ordering::Relaxed) / 1_000_000;
                    eprintln!("    hilos-ms en E/S: {io}, analizando: {parse} (suma de todos los hilos)");
                    let ms = t0.elapsed().as_millis() as u64;
                    let gbs = s.stats.bytes_read as f64 / 1e9 / (s.stats.read_ms.max(1) as f64 / 1000.0);
                    eprintln!(
                        "{} pasada {}: total {} ms (lectura {} ms a {:.2} GB/s, árbol {} ms) — {} archivos",
                        if unbuffered { "sin caché" } else { "con caché" },
                        i + 1,
                        ms,
                        s.stats.read_ms,
                        gbs,
                        s.stats.build_ms,
                        s.stats.files
                    );
                    results.push(serde_json::json!({"unbuffered": unbuffered, "ms": ms, "read_ms": s.stats.read_ms, "build_ms": s.stats.build_ms, "gbs": gbs, "files": s.stats.files, "nodes": s.tree.len()}));
                }
            }
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        #[cfg(windows)]
        "readbench" => {
            // desvan readbench C: → prueba combinaciones de hilos y bloque, sin analizar
            let target = a.target.clone().context("indica una unidad (C:)")?;
            let letter = target.chars().next().context("unidad vacía")?;
            for unbuffered in [true, false] {
                for &(threads, block_mb) in &[(1usize, 4usize), (4, 4), (8, 4), (16, 4), (32, 4), (8, 1), (32, 1), (8, 16), (4, 64)] {
                    let src = FileSource::open_volume_with(letter, unbuffered)?;
                    let (bytes, ms) = ntfs::scan::read_bench(&src, threads, block_mb << 20)?;
                    eprintln!(
                        "{:9} hilos {:>2} bloque {:>2} MB: {:.2} GB en {} ms → {:.2} GB/s",
                        if unbuffered { "sin caché" } else { "con caché" },
                        threads,
                        block_mb,
                        bytes as f64 / 1e9,
                        ms,
                        bytes as f64 / 1e9 / (ms.max(1) as f64 / 1000.0)
                    );
                }
            }
        }
        _ => {
            println!("uso: desvan scan <C:> | --image <archivo.img> [--top N] [--json] [--repeat N]
     desvan bench <C:> [--repeat N]");
        }
    }
    Ok(())
}
