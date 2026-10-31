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
        _ => {
            println!("uso: desvan scan <C:> | --image <archivo.img> [--top N] [--json] [--repeat N]");
        }
    }
    Ok(())
}
