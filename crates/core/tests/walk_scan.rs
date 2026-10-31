//! Pruebas del recorrido de carpetas sobre árboles temporales reales.

use desvan_core::walk::{scan, scan_with, WalkOptions};
use desvan_core::{flags, NodeId, Phase, Progress, Scan, Tree, NONE};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;

/// Carpeta temporal que se borra al terminar.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("desvan-walk-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn find(t: &Tree, name: &str) -> NodeId {
    (0..t.len() as NodeId).find(|&n| t.name(n) == name).unwrap_or(NONE)
}

fn has(t: &Tree, n: NodeId, f: u16) -> bool {
    t.flags[n as usize] & f != 0
}

fn write(p: &Path, len: usize, seed: u8) {
    let data: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect();
    fs::write(p, data).unwrap();
}

fn walk(p: &Path) -> Scan {
    scan(p, &Progress::default()).unwrap()
}

#[test]
fn nested_dirs_totals_and_progress() {
    let d = TempDir::new("nested");
    let r = d.path();
    fs::create_dir_all(r.join("a/b/c/d")).unwrap();
    fs::create_dir_all(r.join("vacía/también")).unwrap();
    fs::create_dir_all(r.join("x")).unwrap();
    write(&r.join("a/uno.bin"), 10_000, 1);
    write(&r.join("a/b/dos.bin"), 100_000, 2);
    write(&r.join("a/b/c/d/tres.bin"), 1_000_000, 3);
    write(&r.join("x/cuatro.txt"), 12, 4);
    write(&r.join("cero.dat"), 0, 5);

    let p = Progress::default();
    let s = scan(r, &p).unwrap();
    let t = &s.tree;
    let root = t.root as usize;
    assert_eq!(s.method, desvan_core::Method::Walk);
    assert_eq!(t.files[root], 5);
    assert_eq!(t.dirs[root], 7);
    assert_eq!(t.size[root], 1_110_012);
    assert_eq!((s.stats.files, s.stats.dirs, s.stats.denied), (5, 8, 0)); // dirs incluye la raíz
    assert!(s.volume.is_none(), "una subcarpeta no es la raíz del volumen");
    assert_eq!(s.root_path, r.to_string_lossy().trim_end_matches('\\'));
    assert_eq!(t.name(t.root), s.root_path);

    // tamaño en disco: múltiplo del sector, nunca menos que el contenido (salvo residentes)
    for name in ["uno.bin", "dos.bin", "tres.bin"] {
        let n = find(t, name) as usize;
        assert!(t.alloc[n] >= t.size[n] && t.alloc[n] - t.size[n] < 64 * 1024, "{name}: {} vs {}", t.alloc[n], t.size[n]);
        assert_eq!(t.alloc[n] % 512, 0);
    }
    assert_eq!(t.alloc[find(t, "cero.dat") as usize], 0);
    assert!(t.alloc[root] >= 1_110_000);

    // rutas y orden
    let tres = find(t, "tres.bin");
    assert_eq!(t.path(tres, &s.root_path), format!("{}\\a\\b\\c\\d\\tres.bin", s.root_path));
    assert_eq!(t.name(t.children(t.root)[0]), "a", "los hijos van de mayor a menor");

    // progreso
    let ps = p.snapshot();
    assert_eq!(ps.phase, Phase::Done);
    assert_eq!(ps.done, 8);
    assert_eq!(ps.total, 8);
    assert_eq!(ps.files, 5);
    assert!(ps.bytes >= 1_110_000);
}

#[test]
fn unicode_names() {
    let d = TempDir::new("unicode");
    let names = ["ñandú.txt", "日本語のファイル.dat", "emoji 📁 carpeta", "Ωmega ünïcödé", "مرحبا.bin"];
    for (i, n) in names.iter().enumerate() {
        if n.contains('.') {
            write(&d.path().join(n), 100 + i, i as u8);
        } else {
            fs::create_dir(d.path().join(n)).unwrap();
            write(&d.path().join(n).join("dentro€.txt"), 50, 9);
        }
    }
    let s = walk(d.path());
    let t = &s.tree;
    for n in names {
        let id = find(t, n);
        assert_ne!(id, NONE, "falta {n}");
        let p = t.path(id, &s.root_path);
        #[cfg(not(windows))]
        let p = p.replace('\\', "/");
        assert!(Path::new(&p).exists(), "{p}");
    }
    assert_ne!(find(t, "dentro€.txt"), NONE);
}

#[test]
fn long_paths_over_260() {
    let d = TempDir::new("long");
    let mut p = d.path().to_path_buf();
    for i in 0..12 {
        p.push(format!("carpeta_con_nombre_bastante_largo_{i:02}"));
    }
    fs::create_dir_all(&p).unwrap();
    write(&p.join("hondo.bin"), 4321, 7);
    let s = walk(d.path());
    let t = &s.tree;
    let n = find(t, "hondo.bin");
    assert_ne!(n, NONE);
    let full = t.path(n, &s.root_path);
    assert!(full.len() > 260, "{}", full.len());
    assert_eq!(t.depth(n), 13);
    assert_eq!(t.size[t.root as usize], 4321);
}

#[test]
fn hard_links_count_once() {
    let d = TempDir::new("links");
    let r = d.path();
    fs::create_dir(r.join("otra")).unwrap();
    write(&r.join("original.bin"), 200_000, 1);
    fs::hard_link(r.join("original.bin"), r.join("otra/enlace.bin")).unwrap();
    write(&r.join("normal.bin"), 50_000, 2);

    let s = walk(r);
    let t = &s.tree;
    let (a, b) = (find(t, "original.bin"), find(t, "enlace.bin"));
    assert_eq!(t.key[a as usize], t.key[b as usize], "los enlaces duros comparten clave");
    let secondary = [a, b].iter().filter(|&&n| has(t, n, flags::HARDLINK)).count();
    #[cfg(windows)]
    {
        // NTFS: el id de archivo delata el enlace y el contenido se cuenta una vez
        assert_eq!(secondary, 1);
        assert_eq!(t.size[t.root as usize], 250_000);
    }
    #[cfg(unix)]
    assert_eq!(secondary, 1);
    assert_eq!(t.files[t.root as usize], 3);

    // con FindFirstFileExW no hay ids: cada enlace cuenta (limitación conocida)
    #[cfg(windows)]
    {
        let s = scan_with(r, &Progress::default(), &WalkOptions { force_find_first: true, ..Default::default() }).unwrap();
        let t = &s.tree;
        assert_eq!(t.size[t.root as usize], 450_000);
        assert!(!(0..t.len()).any(|i| t.flags[i] & flags::HARDLINK != 0));
    }
}

#[test]
fn both_listing_modes_agree() {
    let d = TempDir::new("modes");
    let r = d.path();
    for i in 0..30 {
        let sub = r.join(format!("d{}", i % 7)).join(format!("e{}", i % 3));
        fs::create_dir_all(&sub).unwrap();
        write(&sub.join(format!("f{i}.bin")), 1000 * i + 1, i as u8);
    }
    let a = scan_with(r, &Progress::default(), &WalkOptions { threads: 4, ..Default::default() }).unwrap();
    let b = scan_with(r, &Progress::default(), &WalkOptions { threads: 2, force_find_first: true }).unwrap();
    let (ta, tb) = (&a.tree, &b.tree);
    assert_eq!(ta.files[ta.root as usize], 30);
    assert_eq!(ta.files[ta.root as usize], tb.files[tb.root as usize]);
    assert_eq!(ta.dirs[ta.root as usize], tb.dirs[tb.root as usize]);
    assert_eq!(ta.size[ta.root as usize], tb.size[tb.root as usize]);
    // claves densas: 1 raíz + 28 carpetas + 30 archivos
    let max_key = ta.key.iter().copied().filter(|&k| k != u64::MAX).max().unwrap();
    assert!(max_key < 64, "claves no densas: {max_key}");
}

#[test]
fn cancelled_scan_returns_error() {
    let d = TempDir::new("cancel");
    fs::create_dir_all(d.path().join("a/b")).unwrap();
    let p = Progress::default();
    p.cancel.store(true, Relaxed);
    assert!(matches!(scan(d.path(), &p), Err(desvan_core::Error::Cancelled)));
}

#[cfg(windows)]
mod windows_only {
    use super::*;
    use std::process::Command;

    fn cmd(args: &[&str]) -> bool {
        Command::new("cmd").arg("/c").args(args).output().map(|o| o.status.success()).unwrap_or(false)
    }

    #[test]
    fn junctions_and_symlinks_are_not_followed() {
        let d = TempDir::new("junction");
        let r = d.path();
        fs::create_dir_all(r.join("destino/sub")).unwrap();
        write(&r.join("destino/sub/grande.bin"), 300_000, 1);
        let link = r.join("union");
        // las uniones no necesitan permisos de administrador
        if !cmd(&["mklink", "/J", &link.to_string_lossy(), &r.join("destino").to_string_lossy()]) {
            eprintln!("mklink /J no disponible: se omite");
            return;
        }
        // enlace simbólico de archivo: solo con modo desarrollador o administrador
        let sym = std::os::windows::fs::symlink_file(r.join("destino/sub/grande.bin"), r.join("simbolico.bin")).is_ok();

        let s = walk(r);
        let t = &s.tree;
        let j = find(t, "union");
        assert_ne!(j, NONE);
        assert!(has(t, j, flags::REPARSE) && has(t, j, flags::DIR));
        assert!(t.children(j).is_empty(), "no se baja por la unión");
        assert_eq!(t.alloc[j as usize], 0);
        assert_eq!(t.size[t.root as usize], 300_000, "el destino se cuenta una sola vez");
        assert_eq!(t.files[t.root as usize], 1 + sym as u32);
        if sym {
            let n = find(t, "simbolico.bin");
            assert!(has(t, n, flags::REPARSE) && !has(t, n, flags::DIR));
            assert_eq!(t.alloc[n as usize], 0);
        }
        // borrar la unión no toca el destino
        fs::remove_dir(&link).unwrap();
        assert!(r.join("destino/sub/grande.bin").exists());
    }

    #[test]
    fn denied_folder_is_marked_and_scan_continues() {
        let d = TempDir::new("denied");
        let r = d.path();
        let locked = r.join("cerrada");
        fs::create_dir_all(locked.join("dentro")).unwrap();
        write(&locked.join("secreto.bin"), 5000, 1);
        write(&r.join("visible.bin"), 7000, 2);
        // quita a Todos el permiso de listar (el dueño puede devolverlo)
        let ok = Command::new("icacls").arg(&locked).args(["/deny", "*S-1-1-0:(RD)"]).output().map(|o| o.status.success()).unwrap_or(false);
        if !ok {
            eprintln!("icacls no disponible: se omite");
            return;
        }
        let s = walk(r);
        // si la carpeta denegada es la propia raíz, es un error (no un árbol vacío)
        let root_err = scan(&locked, &Progress::default());
        let _ = Command::new("icacls").arg(&locked).args(["/remove:d", "*S-1-1-0"]).output();
        let t = &s.tree;
        let n = find(t, "cerrada");
        assert!(has(t, n, flags::DENIED));
        assert!(t.children(n).is_empty());
        assert_eq!(s.stats.denied, 1);
        assert_ne!(find(t, "visible.bin"), NONE);
        assert_eq!(t.size[t.root as usize], 7000);
        match root_err {
            Err(desvan_core::Error::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied),
            other => panic!("se esperaba acceso denegado: {:?}", other.map(|s| s.stats)),
        }
    }

    #[test]
    fn compressed_sparse_and_offline_files() {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_OFFLINE};
        let d = TempDir::new("special");
        let r = d.path();
        let text: Vec<u8> = b"linea repetida para que comprima muy bien\n".iter().copied().cycle().take(1_200_000).collect();
        fs::write(r.join("comprimido.txt"), &text).unwrap();
        fs::write(r.join("wof.txt"), &text).unwrap();
        fs::write(r.join("normal.txt"), &text).unwrap();
        let compact = |args: &[&str]| Command::new("compact").args(args).current_dir(r).output().map(|o| o.status.success()).unwrap_or(false);
        let ntfs_c = compact(&["/c", "comprimido.txt"]);
        let wof = compact(&["/c", "/exe:xpress4k", "wof.txt"]);

        // disperso: 100 MB lógicos, casi nada escrito
        let sparse = make_sparse(&r.join("disperso.bin"), 100 << 20);
        eprintln!("comprimido NTFS: {ntfs_c}, WOF: {wof}, disperso: {sparse}");

        // desconectado (como un marcador de la nube): cuenta 0 y no se abre
        write(&r.join("nube.bin"), 50_000, 3);
        let w: Vec<u16> = r.join("nube.bin").to_string_lossy().encode_utf16().chain([0]).collect();
        // SAFETY: ruta con nulo final
        unsafe { SetFileAttributesW(PCWSTR(w.as_ptr()), FILE_ATTRIBUTE_ARCHIVE | FILE_ATTRIBUTE_OFFLINE) }.unwrap();

        let s = walk(r);
        let t = &s.tree;
        let normal = find(t, "normal.txt") as usize;
        assert!(t.alloc[normal] >= 1_200_000);
        if ntfs_c {
            let n = find(t, "comprimido.txt");
            assert!(has(t, n, flags::COMPRESSED));
            assert!(t.alloc[n as usize] < 600_000, "comprimido: {}", t.alloc[n as usize]);
        }
        if wof {
            // WOF oculta su punto de reparse y el directorio dice 0: hay que preguntar
            let n = find(t, "wof.txt") as usize;
            assert!(t.alloc[n] > 0 && t.alloc[n] < 600_000, "wof: {}", t.alloc[n]);
            assert_eq!(t.size[n], 1_200_000);
        }
        if sparse {
            let n = find(t, "disperso.bin");
            assert!(has(t, n, flags::SPARSE));
            assert_eq!(t.size[n as usize], (100 << 20) + 3);
            assert!(t.alloc[n as usize] <= 1 << 20, "disperso: {}", t.alloc[n as usize]);
        }
        let n = find(t, "nube.bin");
        assert!(has(t, n, flags::OFFLINE));
        assert_eq!(t.alloc[n as usize], 0);
        assert_eq!(t.size[n as usize], 50_000);

        // en modo FindFirstFileExW los comprimidos y dispersos se piden aparte
        let s = scan_with(r, &Progress::default(), &WalkOptions { force_find_first: true, ..Default::default() }).unwrap();
        let t = &s.tree;
        if ntfs_c {
            assert!(t.alloc[find(t, "comprimido.txt") as usize] < 600_000);
        }
        if sparse {
            assert!(t.alloc[find(t, "disperso.bin") as usize] <= 1 << 20);
        }
        assert_eq!(t.alloc[find(t, "nube.bin") as usize], 0);
    }

    fn make_sparse(path: &Path, len: u64) -> bool {
        use std::io::{Seek, SeekFrom, Write};
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Ioctl::FSCTL_SET_SPARSE;
        use windows::Win32::System::IO::DeviceIoControl;
        let Ok(mut f) = fs::File::create(path) else { return false };
        let mut ret = 0u32;
        // SAFETY: handle válido del File
        let ok = unsafe { DeviceIoControl(HANDLE(f.as_raw_handle()), FSCTL_SET_SPARSE, None, 0, None, 0, Some(&mut ret), None) }.is_ok();
        ok && f.seek(SeekFrom::Start(len)).is_ok() && f.write_all(b"fin").is_ok()
    }
}
