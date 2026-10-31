//! Pruebas de las sugerencias de limpieza sobre árboles sintéticos (sin disco).

use desvan_core::junk::{find_junk, find_junk_at, JunkHit, Risk};
use desvan_core::{flags, Builder, NodeId, Tree};
use std::collections::HashMap;

const NOW: i64 = 1_800_000_000;
const DAY: i64 = 86_400;
const OLD: i64 = NOW - 100 * DAY;
const RECENT: i64 = NOW - 5 * DAY;

/// Construye un árbol a partir de rutas; las carpetas intermedias se crean solas.
struct Spec {
    b: Builder,
    dirs: HashMap<String, u64>,
    next: u64,
}

impl Spec {
    fn new() -> Self {
        let mut b = Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        Spec { b, dirs: HashMap::from([(String::new(), 1)]), next: 2 }
    }

    fn split(path: &str) -> (&str, &str) {
        match path.rfind('\\') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path),
        }
    }

    fn dir_at(&mut self, path: &str, mtime: i64) -> u64 {
        if let Some(&k) = self.dirs.get(&path.to_lowercase()) {
            return k;
        }
        let (parent, name) = Self::split(path);
        let pk = self.dir_at(parent, mtime);
        let k = self.next;
        self.next += 1;
        self.b.add(k, pk, name, 0, 0, mtime, flags::DIR);
        self.dirs.insert(path.to_lowercase(), k);
        k
    }

    fn dir(mut self, path: &str, mtime: i64) -> Self {
        self.dir_at(path, mtime);
        self
    }

    fn file_full(mut self, path: &str, bytes: u64, mtime: i64, key: Option<u64>, fl: u16) -> Self {
        let (parent, name) = Self::split(path);
        let pk = self.dir_at(parent, OLD);
        let k = key.unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        });
        self.b.add(k, pk, name, bytes, bytes, mtime, fl);
        self
    }

    fn file(self, path: &str, bytes: u64) -> Self {
        self.file_full(path, bytes, OLD, None, 0)
    }

    fn file_at(self, path: &str, bytes: u64, mtime: i64) -> Self {
        self.file_full(path, bytes, mtime, None, 0)
    }

    fn build(self, root: &str) -> Tree {
        self.b.build(1, root)
    }
}

fn hit<'a>(hits: &'a [JunkHit], rule: &str) -> Option<&'a JunkHit> {
    hits.iter().find(|h| h.rule == rule)
}

fn paths(t: &Tree, root: &str, h: &JunkHit) -> Vec<String> {
    let mut v: Vec<String> = h.nodes.iter().map(|&n| t.path(n, root)).collect();
    v.sort();
    v
}

#[test]
fn windows_volume_rules() {
    let t = Spec::new()
        .file(r"Users\ana\AppData\Local\Temp\x.tmp", 1000)
        .file(r"Users\bob\AppData\Local\Temp\sub\y.tmp", 2000)
        .file(r"Windows\Temp\z.log", 500)
        .file(r"Windows\SoftwareDistribution\Download\abc\upd.cab", 7000)
        .file(r"Windows\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache\p", 600)
        .file(r"$Recycle.Bin\S-1-5-21-1\$RABC.docx", 4000)
        .file(r"Windows.old\Windows\System32\viejo.dll", 9000)
        .file("hiberfil.sys", 12_000)
        .file("pagefile.sys", 8000)
        .file("swapfile.sys", 100)
        .file(r"Windows\MEMORY.DMP", 3000)
        .file(r"Windows\Minidump\103126-1.dmp", 60)
        .file(r"Users\ana\AppData\Local\CrashDumps\app.exe.123.dmp", 100)
        .file(r"Datos\proyecto\cuelgue.dmp", 50)
        .file(r"Users\ana\AppData\Local\Microsoft\Windows\Explorer\thumbcache_256.db", 400)
        .file(r"Users\ana\AppData\Local\Microsoft\Windows\Explorer\iconcache_32.db", 100)
        .file(r"Users\ana\AppData\Local\Microsoft\Windows\Explorer\otra.db", 999)
        .file(r"Users\ana\AppData\Local\IconCache.db", 30)
        .file(r"Users\ana\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data\f_0001", 100)
        .file(r"Users\ana\AppData\Local\Google\Chrome\User Data\Profile 1\Code Cache\js\x", 200)
        .file(r"Users\ana\AppData\Local\Google\Chrome\User Data\Default\History", 5555)
        .file(r"Users\ana\AppData\Local\Microsoft\Edge\User Data\Default\GPUCache\data_1", 300)
        .file(r"Users\ana\AppData\Local\Mozilla\Firefox\Profiles\abc.default-release\cache2\entries\X", 400)
        .file(r"FOUND.000\FILE0000.CHK", 700)
        .file(r"System Volume Information\{3808876b-c176}", 5000)
        .file(r"Windows\WinSxS\amd64_algo\algo.dll", 800)
        .file(r"Windows\Logs\CBS\CBS.log", 650)
        .file(r"Windows\Installer\$PatchCache$\Managed\x.msp", 320)
        .file(r"Windows\Installer\12345.msi", 777)
        .file(r"$WINDOWS.~BT\Sources\install.esd", 1500)
        .file(r"NVIDIA\DisplayDriver\560.94\setup.exe", 444)
        .file(r"Users\ana\Documents\informe.docx", 123_456)
        .build(r"C:\");
    let hits = find_junk_at(&t, r"C:\", NOW);
    let expect = [
        ("temp_usuario", 3000, 2, Risk::Seguro),
        ("temp_windows", 500, 1, Risk::Seguro),
        ("windows_update", 7000, 1, Risk::Seguro),
        ("optimizacion_entrega", 600, 1, Risk::Seguro),
        ("papelera", 4000, 1, Risk::Revisar),
        ("windows_old", 9000, 1, Risk::Revisar),
        ("hiberfil", 12_000, 1, Risk::Informativo),
        ("pagefile", 8000, 1, Risk::Informativo),
        ("swapfile", 100, 1, Risk::Informativo),
        ("volcados", 3210, 4, Risk::Seguro),
        ("miniaturas", 530, 3, Risk::Seguro),
        ("cache_navegadores", 1000, 4, Risk::Seguro),
        ("chkdsk", 700, 1, Risk::Revisar),
        ("restauracion", 5000, 1, Risk::Informativo),
        ("winsxs", 800, 1, Risk::Informativo),
        ("registros_windows", 650, 1, Risk::Revisar),
        ("parches_instalador", 320, 1, Risk::Revisar),
        ("restos_actualizacion", 1500, 1, Risk::Revisar),
        ("instaladores_graficos", 444, 1, Risk::Revisar),
    ];
    for (rule, bytes, n, risk) in expect {
        let h = hit(&hits, rule).unwrap_or_else(|| panic!("falta {rule}"));
        assert_eq!((h.bytes, h.nodes.len(), h.risk), (bytes, n, risk), "{rule}: {:?}", paths(&t, r"C:\", h));
        assert!(!h.title.is_empty() && h.description.len() > 40);
    }
    assert_eq!(hits.len(), expect.len(), "sobran: {:?}", hits.iter().map(|h| h.rule).collect::<Vec<_>>());
    // nada de documentos ni del resto de Windows\Installer
    for h in &hits {
        for p in paths(&t, r"C:\", h) {
            assert!(!p.contains("informe.docx") && !p.contains("12345.msi") && !p.contains("History"), "{p}");
        }
    }
    // de mayor a menor
    assert!(hits.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    let temp = hit(&hits, "temp_usuario").unwrap();
    assert_eq!(paths(&t, r"C:\", temp), vec![r"C:\Users\ana\AppData\Local\Temp", r"C:\Users\bob\AppData\Local\Temp"]);
}

#[test]
fn scanning_a_subfolder_still_resolves_users() {
    let t = Spec::new()
        .file(r"AppData\Local\Temp\x.tmp", 1000)
        .file_at(r"Downloads\setup.exe", 2000, OLD)
        .file_at(r"Downloads\nuevo.msi", 3000, RECENT)
        .file_at(r"Downloads\foto.jpg", 4000, OLD)
        .file_at(r"Downloads\sub\imagen.iso", 5000, OLD)
        .file_at(r"Downloads\paquete.ZIP", 600, OLD)
        .build(r"C:\Users\ana");
    let hits = find_junk_at(&t, r"C:\Users\ana", NOW);
    assert_eq!(hit(&hits, "temp_usuario").unwrap().bytes, 1000);
    let dl = hit(&hits, "descargas_instaladores").unwrap();
    assert_eq!(dl.risk, Risk::Revisar);
    assert_eq!(paths(&t, r"C:\Users\ana", dl), vec![r"C:\Users\ana\Downloads\paquete.ZIP", r"C:\Users\ana\Downloads\setup.exe", r"C:\Users\ana\Downloads\sub\imagen.iso"]);
    assert_eq!(dl.bytes, 7600);
    // un recurso de red que es la raíz de un volumen (\\pc\c$) también vale
    let t = Spec::new().file(r"Users\ana\AppData\Local\Temp\x.tmp", 10).build(r"\\pc\c$");
    assert!(hit(&find_junk_at(&t, r"\\pc\c$", NOW), "temp_usuario").is_some());
    // pero un recurso cualquiera no se sabe dónde está en el servidor: no se adivina
    let t = Spec::new().file(r"ana\AppData\Local\Temp\x.tmp", 10).build(r"\\nas\Users");
    assert!(find_junk_at(&t, r"\\nas\Users", NOW).is_empty());
    // y si la raíz escaneada ya es la carpeta Temp, la sugerencia es la raíz entera
    let t = Spec::new().file("a.tmp", 10).file(r"b\c.tmp", 20).build(r"C:\Users\ana\AppData\Local\Temp");
    let hits = find_junk_at(&t, r"C:\Users\ana\AppData\Local\Temp", NOW);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].nodes, vec![t.root]);
    assert_eq!(hits[0].bytes, 30);
}

#[test]
fn developer_caches() {
    let t = Spec::new()
        .file(r"Users\dev\AppData\Local\npm-cache\_cacache\x", 100)
        .file(r"Users\dev\AppData\Local\Yarn\Cache\v6\y", 200)
        .file(r"Users\dev\AppData\Local\pnpm\store\v3\z", 300)
        .file(r"Users\dev\.gradle\caches\modules-2\a.jar", 400)
        .file(r"Users\dev\AppData\Local\pip\cache\http\b", 500)
        .file(r"Users\dev\.nuget\packages\newtonsoft.json\c.nupkg", 600)
        .file(r"Users\dev\AppData\Local\NuGet\v3-cache\d", 700)
        .file(r"Users\dev\AppData\Local\go-build\00\e", 800)
        .file(r"Users\dev\go\pkg\mod\github.com\f", 900)
        .file(r"Users\dev\.cargo\registry\cache\index\serde.crate", 1000)
        .file(r"Users\dev\.cargo\bin\cargo.exe", 1111)
        .build(r"C:\");
    let hits = find_junk_at(&t, r"C:\", NOW);
    let expect = [
        ("npm_cache", 100, Risk::Seguro),
        ("yarn_cache", 200, Risk::Seguro),
        ("pnpm_store", 300, Risk::Revisar),
        ("gradle_cache", 400, Risk::Seguro),
        ("pip_cache", 500, Risk::Seguro),
        ("nuget_paquetes", 600, Risk::Revisar),
        ("nuget_cache", 700, Risk::Seguro),
        ("go_cache", 800, Risk::Seguro),
        ("go_modulos", 900, Risk::Revisar),
        ("cargo_registro", 1000, Risk::Revisar),
    ];
    for (rule, bytes, risk) in expect {
        let h = hit(&hits, rule).unwrap_or_else(|| panic!("falta {rule}"));
        assert_eq!((h.bytes, h.risk), (bytes, risk), "{rule}");
    }
    assert_eq!(hits.len(), expect.len());
}

#[test]
fn node_modules_by_age_and_rust_targets() {
    let t = Spec::new()
        .dir(r"src\viejo", OLD)
        .dir(r"src\viejo\node_modules", OLD)
        .file(r"src\viejo\node_modules\left-pad\index.js", 1000)
        .file(r"src\viejo\node_modules\a\node_modules\b\x.js", 500)
        .dir(r"src\activo", OLD)
        .dir(r"src\activo\node_modules", OLD)
        .dir(r"src\activo\node_modules\react", RECENT)
        .file(r"src\activo\node_modules\react\index.js", 2000)
        .file(r"src\rusty\Cargo.toml", 1)
        .file(r"src\rusty\target\CACHEDIR.TAG", 1)
        .file(r"src\rusty\target\debug\rusty.exe", 9000)
        .file(r"src\otro\target\no-es-rust.bin", 7000)
        .file(r"src\raro\Cargo.toml", 1)
        .file(r"src\raro\target\cosas.txt", 6000)
        .build(r"D:\");
    let hits = find_junk_at(&t, r"D:\", NOW);
    let nm = hit(&hits, "node_modules").unwrap();
    assert_eq!(paths(&t, r"D:\", nm), vec![r"D:\src\viejo\node_modules"]);
    assert_eq!(nm.bytes, 1500, "incluye los node_modules anidados una sola vez");
    let rt = hit(&hits, "rust_target").unwrap();
    assert_eq!(paths(&t, r"D:\", rt), vec![r"D:\src\rusty\target"]);
    assert_eq!(rt.bytes, 9001);
    assert_eq!(rt.risk, Risk::Revisar);
    assert_eq!(hits.len(), 2);
}

#[test]
fn game_engine_projects() {
    let t = Spec::new()
        .file(r"Juegos\Nave\Nave.uproject", 2)
        .file(r"Juegos\Nave\DerivedDataCache\a.udd", 1000)
        .file(r"Juegos\Nave\Intermediate\Build\b.obj", 2000)
        .file(r"Juegos\Nave\Saved\Logs\Nave.log", 30)
        .file(r"Juegos\Nave\Saved\Crashes\c\dump.dmp", 40)
        .file(r"Juegos\Nave\Saved\Config\Game.ini", 5)
        .file(r"Juegos\Nave\Binaries\Win64\Nave.exe", 3000)
        .file(r"Juegos\Nave\Plugins\Efectos\Intermediate\x.obj", 400)
        .file(r"Juegos\Nave\Plugins\Efectos\Binaries\Win64\e.dll", 500)
        .file(r"Juegos\Nave\Content\Mapa.umap", 99_999)
        .file(r"Juegos\SinProyecto\Intermediate\nada.obj", 777)
        .file(r"Users\ana\AppData\Local\UnrealEngine\Common\DerivedDataCache\d.udd", 5000)
        .file(r"Users\ana\AppData\Local\UnrealEngine\Common\Zen\Data\cas\e", 600)
        .file(r"Godot\Mundo\project.godot", 1)
        .file(r"Godot\Mundo\.godot\imported\icono.ctex", 70)
        .file(r"Godot\Mundo\.godot\editor\estado.cfg", 7)
        .file(r"Godot\Viejo\project.godot", 1)
        .file(r"Godot\Viejo\.import\a.stex", 80)
        .dir(r"Unity\Granja\Assets", OLD)
        .dir(r"Unity\Granja\ProjectSettings", OLD)
        .file(r"Unity\Granja\Library\ArtifactDB", 900)
        .file(r"Otros\Library\libro.pdf", 1234)
        .file(r"SteamLibrary\steamapps\shadercache\440\fozpipelinesv6\x", 111)
        .file(r"SteamLibrary\steamapps\common\Juego\juego.exe", 22_222)
        .file(r"Program Files (x86)\Steam\steam.exe", 3)
        .file(r"Program Files (x86)\Steam\config\htmlcache\Cache\f", 222)
        .file(r"Program Files (x86)\Steam\config\config.vdf", 4)
        .file(r"Users\ana\AppData\Local\EpicGamesLauncher\Saved\webcache_4147\f", 333)
        .file(r"Users\ana\AppData\Local\NVIDIA\DXCache\a.nvph", 44)
        .file(r"Users\ana\AppData\Local\D3DSCache\b\c.idx", 55)
        .file(r"ProgramData\Epic\EpicGamesLauncher\VaultCache\Pack\data.pak", 666)
        .build(r"E:\");
    let hits = find_junk_at(&t, r"E:\", NOW);
    let p = |rule: &str| paths(&t, r"E:\", hit(&hits, rule).unwrap_or_else(|| panic!("falta {rule}")));
    assert_eq!(p("unreal_cache"), vec![r"E:\Juegos\Nave\DerivedDataCache", r"E:\Juegos\Nave\Intermediate", r"E:\Juegos\Nave\Plugins\Efectos\Intermediate"]);
    assert_eq!(p("unreal_logs"), vec![r"E:\Juegos\Nave\Saved\Crashes", r"E:\Juegos\Nave\Saved\Logs"]);
    assert_eq!(p("unreal_binarios"), vec![r"E:\Juegos\Nave\Binaries"]);
    assert_eq!(hit(&hits, "unreal_binarios").unwrap().risk, Risk::Revisar);
    assert_eq!(hit(&hits, "unreal_ddc_global").unwrap().bytes, 5600);
    assert_eq!(p("godot_import"), vec![r"E:\Godot\Mundo\.godot\imported", r"E:\Godot\Viejo\.import"]);
    assert_eq!(p("unity_library"), vec![r"E:\Unity\Granja\Library"]);
    assert_eq!(p("steam_shaders"), vec![r"E:\SteamLibrary\steamapps\shadercache"]);
    assert_eq!(p("steam_htmlcache"), vec![r"E:\Program Files (x86)\Steam\config\htmlcache"]);
    assert_eq!(p("epic_webcache"), vec![r"E:\Users\ana\AppData\Local\EpicGamesLauncher\Saved\webcache_4147"]);
    assert_eq!(hit(&hits, "cache_shaders_gpu").unwrap().bytes, 99);
    assert_eq!(hit(&hits, "epic_vaultcache").unwrap().risk, Risk::Revisar);
    // el .dmp de Saved\Crashes va con su carpeta, no suelto
    assert!(hit(&hits, "volcados").is_none());
    let all: Vec<String> = hits.iter().flat_map(|h| paths(&t, r"E:\", h)).collect();
    for keep in ["SinProyecto", "Content", "Config", "Efectos\\Binaries", "Otros", "editor", "common"] {
        assert!(!all.iter().any(|p| p.contains(keep)), "no debía tocar {keep}: {all:?}");
    }
}

#[test]
fn electron_caches_need_two_signatures() {
    let t = Spec::new()
        .file(r"Users\ana\AppData\Roaming\discord\Cache\Cache_Data\x", 100)
        .file(r"Users\ana\AppData\Roaming\discord\Code Cache\js\y", 200)
        .file(r"Users\ana\AppData\Roaming\discord\GPUCache\z", 300)
        .file(r"Users\ana\AppData\Roaming\discord\Local Storage\leveldb\keep", 999)
        .file(r"Users\ana\AppData\Roaming\Juego\Cache\partida.sav", 5000)
        .build(r"C:\");
    let hits = find_junk_at(&t, r"C:\", NOW);
    let h = hit(&hits, "cache_aplicaciones").unwrap();
    assert_eq!(h.bytes, 600);
    assert_eq!(h.nodes.len(), 3);
    assert_eq!(hits.len(), 1, "una sola carpeta «Cache» no basta para tocarla");
}

#[test]
fn no_double_counting_hard_links_and_empty() {
    let t = Spec::new()
        .file(r"Users\ana\AppData\Local\Temp\proyecto\node_modules\x.js", 100)
        .file(r"Users\ana\AppData\Local\Temp\volcado.dmp", 50)
        .file_full(r"Users\ana\Downloads\setup.exe", 1000, OLD, Some(500), 0)
        .file_full(r"Users\ana\Downloads\copia-enlace.exe", 1000, OLD, Some(500), 0)
        .dir(r"Users\bob\AppData\Local\Temp", OLD)
        .file_full(r"Users\ana\AppData\Local\Packages\CanonicalGroupLimited.Ubuntu\LocalState\ext4.vhdx", 50_000, OLD, None, 0)
        .file(r"Users\ana\Documents\vm.vhdx", 70_000)
        .build(r"C:\");
    let hits = find_junk_at(&t, r"C:\", NOW);
    let temp = hit(&hits, "temp_usuario").unwrap();
    assert_eq!((temp.bytes, temp.nodes.len()), (150, 1), "la Temp vacía de bob no aparece y lo de dentro no se repite");
    assert!(hit(&hits, "node_modules").is_none() && hit(&hits, "volcados").is_none());
    let dl = hit(&hits, "descargas_instaladores").unwrap();
    assert_eq!((dl.bytes, dl.nodes.len()), (1000, 1), "el enlace duro secundario no suma");
    let v = hit(&hits, "discos_virtuales").unwrap();
    assert_eq!((v.bytes, v.risk), (50_000, Risk::Informativo));
    assert_eq!(hits.len(), 3);
}

#[test]
fn hits_serialize_for_the_ui() {
    let t = Spec::new().file("hiberfil.sys", 10).build(r"C:\");
    let hits = find_junk(&t, r"C:\");
    let j = serde_json::to_string(&hits).unwrap();
    assert!(j.contains("\"risk\":\"informativo\"") && j.contains("\"rule\":\"hiberfil\""), "{j}");
    assert_eq!(format!("{:?}", hits[0].risk).to_lowercase(), "informativo");
    let ids: Vec<NodeId> = hits[0].nodes.clone();
    assert_eq!(t.name(ids[0]), "hiberfil.sys");
}
