//! Compara el lector de la MFT con el manifiesto que genera `scripts/make-ntfs-images.sh`
//! (ntfs-3g montando la misma imagen). Si las imágenes no existen, las pruebas se saltan.

use desvan_core::ntfs::{self, FileSource};
use desvan_core::{flags, NodeId, Progress, Tree};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

fn testdata(name: &str) -> Option<(PathBuf, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let img = dir.join(format!("{name}.img"));
    let json = dir.join(format!("{name}.json"));
    if !img.exists() || !json.exists() {
        eprintln!("(sin {name}.img: ejecuta scripts/make-ntfs-images.sh en WSL)");
        return None;
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(json).unwrap()).unwrap();
    Some((img, v))
}

/// ruta relativa ("Fotos\playa.jpg") -> nodos (varios si hay enlaces duros con el mismo nombre de ruta, no debería)
fn paths(t: &Tree) -> HashMap<String, NodeId> {
    let mut out = HashMap::new();
    let mut stack: Vec<(NodeId, String)> = vec![(t.root, String::new())];
    while let Some((n, prefix)) = stack.pop() {
        for &c in t.children(n) {
            if t.flags[c as usize] & (flags::METAFILE | flags::SYNTHETIC) != 0 {
                continue;
            }
            let p = if prefix.is_empty() { t.name(c).to_string() } else { format!("{prefix}\\{}", t.name(c)) };
            if t.is_dir(c) {
                stack.push((c, p.clone()));
            }
            out.insert(p, c);
        }
    }
    out
}

#[test]
fn basico_matches_ntfs3g() {
    let Some((img, manifest)) = testdata("basico") else { return };
    let src = FileSource::open_image(&img).unwrap();
    let scan = ntfs::scan(&src, "IMG:\\", &Progress::default()).unwrap();
    let t = &scan.tree;
    assert_eq!(scan.stats.bad_records, 0);
    assert_eq!(t.orphans, desvan_core::NONE, "no debería haber huérfanos");

    let ours = paths(t);
    let items = manifest["items"].as_array().unwrap();
    let mut missing = Vec::new();
    let mut wrong = Vec::new();
    let mut by_ino: HashMap<u64, Vec<NodeId>> = HashMap::new();
    for it in items {
        let p = it["path"].as_str().unwrap();
        let Some(&n) = ours.get(p) else {
            missing.push(p.to_string());
            continue;
        };
        let is_dir = it["dir"].as_bool().unwrap();
        if is_dir != t.is_dir(n) {
            wrong.push(format!("{p}: carpeta {} vs {}", is_dir, t.is_dir(n)));
            continue;
        }
        if !is_dir {
            let size = it["size"].as_u64().unwrap();
            if size != t.size[n as usize] {
                wrong.push(format!("{p}: tamaño {} vs {}", size, t.size[n as usize]));
            }
            // el número de inodo de ntfs-3g es el número de registro de la MFT
            assert_eq!(t.key[n as usize], it["ino"].as_u64().unwrap(), "{p}: registro");
            if it["nlink"].as_u64().unwrap() > 1 {
                by_ino.entry(it["ino"].as_u64().unwrap()).or_default().push(n);
            }
        }
    }
    assert!(missing.is_empty(), "faltan {} rutas, p. ej. {:?}", missing.len(), &missing[..missing.len().min(5)]);
    assert!(wrong.is_empty(), "{:?}", &wrong[..wrong.len().min(8)]);

    // cada grupo de enlaces duros tiene exactamente un nombre principal
    assert!(by_ino.len() >= 2, "la imagen debería tener enlaces duros");
    for (ino, nodes) in &by_ino {
        let primary = nodes.iter().filter(|&&n| t.flags[n as usize] & flags::HARDLINK == 0).count();
        assert_eq!(primary, 1, "registro {ino}: {} principales de {}", primary, nodes.len());
    }
    // el archivo de 40 enlaces vive repartido en registros de extensión ($ATTRIBUTE_LIST)
    let shared = by_ino.values().find(|v| v.len() >= 41).expect("grupo de 41 nombres");
    assert!(shared.iter().all(|&n| t.size[n as usize] == 200_000));

    // nada de más: todo lo que vemos (salvo metadatos) existe en el manifiesto
    assert_eq!(ours.len(), items.len(), "rutas de más en el árbol");

    // casos concretos
    let get = |p: &str| ours[p] as usize;
    assert_eq!(t.size[get("Juegos\\disperso.vhd")], 40 * 1024 * 1024);
    assert!(t.alloc[get("Juegos\\disperso.vhd")] <= 256 * 1024, "el disperso solo ocupa sus clústeres reales");
    assert!(t.flags[get("Documentos\\con flujo.txt")] & flags::ADS != 0);
    // tamaño en disco = bloques que informa ntfs-3g (que solo cuenta el flujo principal)
    let mut alloc_wrong = Vec::new();
    for it in items {
        if it["dir"].as_bool().unwrap() {
            continue;
        }
        let n = ours[it["path"].as_str().unwrap()] as usize;
        if t.flags[n] & flags::ADS != 0 {
            continue;
        }
        let blocks = it["blocks"].as_u64().unwrap();
        // datos residentes: Linux informa de un bloque de 512 bytes; en clústeres reales son 0
        let resident = blocks <= 512 && t.alloc[n] == 0;
        if blocks != t.alloc[n] && !resident {
            alloc_wrong.push(format!("{}: {} vs {}", it["path"], blocks, t.alloc[n]));
        }
    }
    assert!(alloc_wrong.is_empty(), "{} diferencias de tamaño en disco: {:?}", alloc_wrong.len(), &alloc_wrong[..alloc_wrong.len().min(6)]);
    assert_eq!(t.files[get("Código\\node_modules")], 60 * 80);
    // el total de la raíz no cuenta dos veces los enlaces
    let sum_children: u64 = t.children(t.root).iter().map(|&c| t.alloc[c as usize]).sum();
    assert_eq!(sum_children + t.alloc[t.root as usize] - sum_children, t.alloc[t.root as usize]);
}

#[test]
fn grande_has_every_file() {
    let Some((img, manifest)) = testdata("grande") else { return };
    let src = FileSource::open_image(&img).unwrap();
    let scan = ntfs::scan(&src, "IMG:\\", &Progress::default()).unwrap();
    let t = &scan.tree;
    let items = manifest["items"].as_array().unwrap();
    let files = items.iter().filter(|i| !i["dir"].as_bool().unwrap()).count() as u64;
    let bytes: u64 = items.iter().filter(|i| !i["dir"].as_bool().unwrap()).map(|i| i["size"].as_u64().unwrap()).sum();
    let ours = paths(t);
    assert_eq!(ours.len(), items.len());
    let non_meta_files: u64 = t.children(t.root).iter().filter(|&&c| t.flags[c as usize] & flags::METAFILE == 0).map(|&c| t.files[c as usize] as u64).sum();
    assert_eq!(non_meta_files, files);
    let non_meta_bytes: u64 = t.children(t.root).iter().filter(|&&c| t.flags[c as usize] & flags::METAFILE == 0).map(|&c| t.size[c as usize]).sum();
    assert_eq!(non_meta_bytes, bytes);
}
