//! Genera un volumen sintético con pinta de Windows real y vuelca su treemap para el
//! laboratorio visual (app/treemap-lab.html).
//!
//! ```text
//! cargo run --release -p desvan-core --example layout_dump -- <salida.bin> <ancho> <alto> [--views N] [--header PX] [--padding PX]
//! ```
//!
//! Escribe `<salida>.bin` (formato de `Layout::to_bytes`) y `<salida>.json` (nombres,
//! tamaños y recuentos por rectángulo), más vistas de subcarpetas (`<salida>.v<id>.*`)
//! para probar el zoom y un índice `<salida>.views.json`.

use desvan_core::categories;
use desvan_core::layout::{self, rflags, Layout, LayoutOpts};
use desvan_core::tree::{flags, Builder, NodeId, Tree};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const GB: u64 = 1024 * MB;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }
    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    /// Tamaño log-normal con mediana `median` y dispersión `sigma`.
    fn size(&mut self, median: u64, sigma: f64) -> u64 {
        let (u1, u2) = (self.unit().max(1e-12), self.unit());
        let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
        ((median as f64).ln() + sigma * z).exp().clamp(1.0, 5e11) as u64
    }
    fn pick<'a>(&mut self, list: &[&'a str]) -> &'a str {
        list[self.below(list.len())]
    }
    fn hex(&mut self, n: usize) -> String {
        (0..n).map(|_| char::from_digit(self.below(16) as u32, 16).unwrap()).collect()
    }
}

struct Gen {
    b: Builder,
    key: u64,
}

/// Lo que ocupa en disco: clústeres de 4 KB; lo muy pequeño vive dentro de la MFT.
fn alloc_of(size: u64) -> u64 {
    if size < 700 {
        0
    } else {
        size.div_ceil(4096) * 4096
    }
}

impl Gen {
    fn next_key(&mut self) -> u64 {
        self.key += 1;
        self.key
    }
    fn dir(&mut self, parent: u64, name: &str) -> u64 {
        self.dir_f(parent, name, 0)
    }
    fn dir_f(&mut self, parent: u64, name: &str, f: u16) -> u64 {
        let k = self.next_key();
        self.b.add(k, parent, name, 0, 0, 1_760_000_000, flags::DIR | f);
        k
    }
    fn file(&mut self, parent: u64, name: &str, size: u64) -> u64 {
        self.file_f(parent, name, size, 0)
    }
    fn file_f(&mut self, parent: u64, name: &str, size: u64, f: u16) -> u64 {
        let k = self.next_key();
        let alloc = if f & flags::OFFLINE != 0 { 0 } else { alloc_of(size) };
        self.b.add(k, parent, name, size, alloc, 1_760_000_000, f);
        k
    }
    /// Enlace duro: misma clave que un archivo ya añadido.
    fn link(&mut self, parent: u64, name: &str, key: u64, size: u64) {
        self.b.add(key, parent, name, size, alloc_of(size), 1_760_000_000, 0);
    }
    /// `n` archivos con nombre `name(i)` y tamaño log-normal.
    fn many(&mut self, r: &mut Rng, parent: u64, n: usize, median: u64, sigma: f64, mut name: impl FnMut(&mut Rng, usize) -> String) {
        for i in 0..n {
            let nm = name(r, i);
            let s = r.size(median, sigma);
            self.file(parent, &nm, s);
        }
    }
    /// Ruta de carpetas (las crea todas) y devuelve la última.
    fn path(&mut self, parent: u64, parts: &[&str]) -> u64 {
        parts.iter().fold(parent, |p, n| self.dir(p, n))
    }
}

const WORDS: &[&str] = &[
    "Storage", "Media", "Graphics", "Security", "Networking", "Shell", "Devices", "Web", "Speech", "Data", "Gaming", "Input", "Audio",
    "Print", "Cloud", "Search", "Sync", "Update", "Display", "Power", "Bluetooth", "Camera", "Ink", "Maps", "Payments", "Store",
    "Identity", "Telemetry", "Diagnostics", "Management", "Wallet", "Holographic", "Widgets", "Copilot", "Photos", "Capture",
];
const CORE_DLLS: &[&str] = &[
    "kernel32", "kernelbase", "ntdll", "user32", "gdi32", "shell32", "combase", "ole32", "oleaut32", "advapi32", "d3d11", "d3d12",
    "dxgi", "dwrite", "msvcp_win", "ucrtbase", "bcrypt", "crypt32", "ws2_32", "winhttp", "wininet", "mfplat", "mfcore", "mshtml",
    "twinui", "ExplorerFrame", "windows.storage", "Windows.UI.Xaml", "rpcrt4", "sechost", "setupapi", "cfgmgr32", "dbghelp",
    "dcomp", "propsys", "uxtheme", "msctf", "wintypes", "coremessaging", "edgehtml", "nvapi64", "nvoglv64", "DriverStore",
];
const GAME_WORDS: &[&str] = &["textures", "audio", "meshes", "animations", "shaders", "maps", "ui", "vfx", "localization", "streaming", "characters", "world"];
const CODE_EXTS: &[&str] = &["js", "js", "js", "json", "md", "ts", "map", "cjs", "mjs", "txt"];
const PERSON: &[&str] = &["Ana", "Lucía", "Pablo", "Marta", "Carlos", "Elena", "Javier", "Sofía", "Diego", "Laura"];

fn dll_name(r: &mut Rng, i: usize) -> String {
    match r.below(4) {
        0 => format!("{}.dll", r.pick(CORE_DLLS)),
        1 => format!("Windows.{}.{}.dll", r.pick(WORDS), r.pick(&["Core", "Runtime", "Service", "Host", "Common", "Api", "Internal"])),
        2 => format!("api-ms-win-{}-l1-1-{}.dll", r.pick(WORDS).to_lowercase(), i % 4),
        _ => format!("{}{}.dll", r.pick(WORDS).to_lowercase(), r.pick(&["svc", "api", "srv", "core", "ui", "host", "ext", "64"])),
    }
}

/// Volumen de ~1 TB con unos 650 000 nodos.
fn build_volume() -> (Tree, u64) {
    let mut g = Gen { b: Builder::with_capacity(700_000), key: 1 };
    let mut r = Rng(20261031);
    let root = 1u64;
    g.b.add(root, 0, "", 0, 0, 0, flags::DIR);
    let sys = flags::SYSTEM | flags::HIDDEN;

    // metadatos de NTFS y archivos de sistema en la raíz
    for (n, s) in [("$MFT", 1_610 * MB), ("$LogFile", 64 * MB), ("$Bitmap", 30 * MB), ("$Secure", 22 * MB), ("$UpCase", 128 * KB)] {
        g.file_f(root, n, s, flags::METAFILE | sys);
    }
    g.file_f(root, "pagefile.sys", 24 * GB, sys);
    g.file_f(root, "hiberfil.sys", 12_900 * MB, sys);
    g.file_f(root, "swapfile.sys", 256 * MB, sys);
    g.file_f(root, "DumpStack.log.tmp", 12 * KB, sys);

    // ---------- Windows ----------
    let win = g.dir(root, "Windows");
    let s32 = g.dir(win, "System32");
    let mut s32_dlls: Vec<(u64, String, u64)> = Vec::new();
    for i in 0..3800 {
        let base = dll_name(&mut r, i);
        let n = if i > 60 { base.replace(".dll", &format!("{}.dll", i % 97)) } else { base };
        let s = r.size(170 * KB, 1.4);
        let k = g.file(s32, &n, s);
        s32_dlls.push((k, n, s));
    }
    g.many(&mut r, s32, 450, 110 * KB, 1.3, |r, i| format!("{}{}.exe", r.pick(WORDS).to_lowercase(), i));
    let drivers = g.dir(s32, "drivers");
    g.many(&mut r, drivers, 520, 60 * KB, 1.3, |r, i| format!("{}{}.sys", r.pick(WORDS).to_lowercase(), i));
    g.file(drivers, "nvlddmkm.sys", 92 * MB);
    let fr = g.path(s32, &["DriverStore", "FileRepository"]);
    for i in 0..700 {
        let h = r.hex(16);
        let name = format!("{}{}.inf_amd64_{h}", r.pick(WORDS).to_lowercase(), i);
        let d = g.dir(fr, &name);
        g.file(d, &format!("{}.inf", &name[..name.find(".inf").unwrap()]), r.size(20 * KB, 0.8));
        g.file(d, "driver.cat", r.size(12 * KB, 0.6));
        let n = r.range(1, 4);
        g.many(&mut r, d, n, 90 * KB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["sys", "dll", "dll", "exe"])));
    }
    let nv = g.dir(fr, "nv_dispig.inf_amd64_3b28e5c6f24d0a91");
    g.many(&mut r, nv, 90, 9 * MB, 1.6, |r, i| format!("nv{}{i}.{}", r.pick(&["ogl", "cuda", "dlss", "ngx", "ml", "ptx"]), r.pick(&["dll", "dll", "bin", "exe"])));
    for loc in ["es-ES", "en-US"] {
        let d = g.dir(s32, loc);
        g.many(&mut r, d, 520, 18 * KB, 0.9, |r, i| format!("{}{i}.dll.mui", r.pick(WORDS).to_lowercase()));
    }
    let cfg = g.dir(s32, "config");
    for (n, s) in [("SYSTEM", 28 * MB), ("SOFTWARE", 142 * MB), ("COMPONENTS", 96 * MB), ("DRIVERS", 9 * MB), ("SAM", 64 * KB), ("SOFTWARE.LOG1", 12 * MB), ("SYSTEM.LOG2", 6 * MB)] {
        g.file(cfg, n, s);
    }
    let cr = g.dir(s32, "catroot2");
    g.file(cr, "catdb", 58 * MB);
    g.file(cr, "edb.log", 1 * MB);
    let lf = g.dir(s32, "LogFiles");
    g.many(&mut r, lf, 140, 400 * KB, 1.6, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["etl", "log", "etl"])));
    let ps = g.path(s32, &["WindowsPowerShell", "v1.0"]);
    g.many(&mut r, ps, 160, 30 * KB, 1.2, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["psd1", "psm1", "dll", "ps1xml"])));

    let wow = g.dir(win, "SysWOW64");
    g.many(&mut r, wow, 2600, 140 * KB, 1.4, dll_name);

    // WinSxS: muchos archivos son enlaces duros de System32 (no suman)
    let sxs = g.dir(win, "WinSxS");
    for i in 0..8500 {
        let name = format!(
            "amd64_microsoft-windows-{}-{}_31bf3856ad364e35_10.0.28000.{}_none_{}",
            r.pick(WORDS).to_lowercase(),
            r.pick(&["core", "service", "runtime", "host", "ui", "deployment"]),
            i % 1300,
            r.hex(16)
        );
        let d = g.dir(sxs, &name);
        if r.chance(0.55) {
            let (k, n, s) = s32_dlls[r.below(s32_dlls.len())].clone();
            g.link(d, &n, k, s);
        }
        let n = r.range(0, 3);
        g.many(&mut r, d, n, 120 * KB, 1.6, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["dll", "dll", "exe", "mui", "sys"])));
    }
    let man = g.dir(sxs, "Manifests");
    g.many(&mut r, man, 8500, 2 * KB, 0.7, |r, i| format!("amd64_{}_{}.manifest", r.pick(WORDS).to_lowercase(), i));
    let inst = g.dir(win, "Installer");
    g.many(&mut r, inst, 260, 8 * MB, 1.4, |r, i| format!("{}{:x}.{}", i, r.next() & 0xffffff, r.pick(&["msi", "msp", "msi"])));
    let sd = g.path(win, &["SoftwareDistribution", "Download"]);
    for _ in 0..14 {
        let h = r.hex(32);
        let d = g.dir(sd, &h);
        let n = r.range(1, 6);
        g.many(&mut r, d, n, 90 * MB, 1.3, |r, i| format!("windows11.0-kb50{}{i}-x64.{}", r.below(99999), r.pick(&["cab", "esd", "psf", "cab"])));
    }
    let ds = g.path(win, &["SoftwareDistribution", "DataStore"]);
    g.file(ds, "DataStore.edb", 410 * MB);
    let fonts = g.dir(win, "Fonts");
    g.many(&mut r, fonts, 700, 260 * KB, 1.3, |r, i| format!("{}{i}.{}", r.pick(&["segoe", "arial", "calibri", "consola", "noto", "yugoth", "msyh", "times"]), r.pick(&["ttf", "ttf", "otf", "ttc"])));
    let pf = g.dir(win, "Prefetch");
    g.many(&mut r, pf, 260, 30 * KB, 0.9, |r, i| format!("{}{i}.EXE-{}.pf", r.pick(WORDS).to_uppercase(), r.hex(8).to_uppercase()));
    let logs = g.path(win, &["Logs", "CBS"]);
    g.file(logs, "CBS.log", 84 * MB);
    g.many(&mut r, logs, 30, 6 * MB, 1.0, |_, i| format!("CbsPersist_2026{:04}.cab", 1000 + i));
    let tmp = g.dir(win, "Temp");
    g.many(&mut r, tmp, 300, 40 * KB, 2.0, |r, _| format!("{}.tmp", r.hex(8)));
    let ni = g.path(win, &["assembly", "NativeImages_v4.0.30319_64"]);
    for i in 0..300 {
        let d = g.dir(ni, &format!("System.{}{i}", r.pick(WORDS)));
        let n = r.range(1, 6);
        g.many(&mut r, d, n, 700 * KB, 1.3, |r, i| format!("{}{i}.ni.dll", r.pick(WORDS)));
    }
    let net = g.path(win, &["Microsoft.NET", "Framework64", "v4.0.30319"]);
    g.many(&mut r, net, 800, 300 * KB, 1.3, |r, i| format!("System.{}{i}.dll", r.pick(WORDS)));
    let svc = g.path(win, &["servicing", "LCU", "Package_for_RollupFix~31bf3856ad364e35~amd64~~28000.1240.1.8"]);
    g.many(&mut r, svc, 2400, 300 * KB, 1.6, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["mum", "cat", "cab", "manifest"])));
    let apps = g.dir(win, "SystemApps");
    for app in ["Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy", "MicrosoftWindows.Client.CBS_cw5n1h2txyewy", "Microsoft.Windows.Search_cw5n1h2txyewy", "MicrosoftWindows.Client.WebExperience_cw5n1h2txyewy"] {
        let d = g.dir(apps, app);
        g.many(&mut r, d, 900, 90 * KB, 1.8, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["js", "dll", "png", "pri", "html", "winmd", "json"])));
    }
    let wall = g.path(win, &["Web", "Wallpaper", "Windows"]);
    g.many(&mut r, wall, 30, 4 * MB, 0.5, |_, i| format!("img{i}.jpg"));
    let inf = g.dir(win, "INF");
    g.many(&mut r, inf, 1800, 25 * KB, 1.3, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["inf", "pnf"])));
    g.file(win, "MEMORY.DMP", 2_430 * MB);
    g.file(win, "explorer.exe", 5 * MB);

    // Windows.old: la instalación anterior, olvidada
    let old = g.dir(root, "Windows.old");
    let os32 = g.path(old, &["Windows", "System32"]);
    g.many(&mut r, os32, 3200, 190 * KB, 1.5, dll_name);
    let osxs = g.path(old, &["Windows", "WinSxS"]);
    for i in 0..1500 {
        let d = g.dir(osxs, &format!("amd64_microsoft-windows-{}_{}", r.pick(WORDS).to_lowercase(), i));
        let n = r.range(1, 4);
        g.many(&mut r, d, n, 400 * KB, 1.7, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["dll", "exe", "mui"])));
    }
    let oapp = g.path(old, &["Users", "usuario", "AppData", "Local"]);
    g.many(&mut r, oapp, 2500, 300 * KB, 2.2, |r, i| format!("{}{i}.{}", r.hex(6), r.pick(&["tmp", "dat", "log", "db", "bin"])));

    // ---------- Program Files ----------
    let pfiles = g.dir(root, "Program Files");
    let ue = g.path(pfiles, &["Epic Games", "UE_5.8", "Engine"]);
    let ub = g.path(ue, &["Binaries", "Win64"]);
    g.many(&mut r, ub, 1200, 1500 * KB, 1.6, |r, i| format!("UnrealEditor-{}{i}.dll", r.pick(WORDS)));
    g.many(&mut r, ub, 400, 24 * MB, 1.2, |r, i| format!("UnrealEditor-{}{i}.pdb", r.pick(WORDS)));
    g.file(ub, "UnrealEditor.exe", 1_100 * KB);
    let uc = g.dir(ue, "Content");
    for w in GAME_WORDS {
        let d = g.dir(uc, &w[..1].to_uppercase().chars().chain(w[1..].chars()).collect::<String>());
        g.many(&mut r, d, 2100, 60 * KB, 1.9, |r, i| format!("{}_{}{i}.{}", r.pick(&["T", "SM", "M", "MI", "BP", "A", "S"]), r.pick(WORDS), if r.chance(0.02) { "umap" } else { "uasset" }));
    }
    let upl = g.dir(ue, "Plugins");
    for i in 0..60 {
        let pn = format!("{}{}{i}", r.pick(WORDS), r.pick(&["Toolkit", "Editor", "Runtime", "Importer", "FX", "Physics"]));
        let p = g.dir(upl, &pn);
        let c = g.dir(p, "Content");
        let n = r.range(20, 400);
        g.many(&mut r, c, n, 120 * KB, 1.9, |r, i| format!("{}{i}.uasset", r.pick(WORDS)));
        let b = g.path(p, &["Binaries", "Win64"]);
        let n = r.range(1, 8);
        g.many(&mut r, b, n, 6 * MB, 1.5, |r, i| format!("UnrealEditor-{}{i}.{}", r.pick(WORDS), r.pick(&["dll", "pdb"])));
    }
    let src = g.dir(ue, "Source");
    for w in ["Runtime", "Editor", "Developer", "Programs", "ThirdParty"] {
        let d = g.dir(src, w);
        for j in 0..40 {
            let m = g.dir(d, &format!("{}{j}", r.pick(WORDS)));
            let n = r.range(50, 400);
            g.many(&mut r, m, n, 9 * KB, 1.2, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["h", "cpp", "cpp", "h", "cs"])));
        }
    }
    let sh = g.dir(ue, "Shaders");
    g.many(&mut r, sh, 3000, 12 * KB, 1.2, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["usf", "ush"])));
    let ddc = g.dir(ue, "DerivedDataCache");
    g.file(ddc, "Compressed.ddp", 5_200 * MB);
    let fp = g.dir(ue, "FeaturePacks");
    g.many(&mut r, fp, 12, 180 * MB, 1.0, |r, i| format!("{}{i}.upack", r.pick(WORDS)));

    let vs = g.path(pfiles, &["Microsoft Visual Studio", "2022", "Community"]);
    let ide = g.path(vs, &["Common7", "IDE"]);
    for i in 0..30 {
        let d = g.dir(ide, &format!("{}{i}", r.pick(WORDS)));
        g.many(&mut r, d, 300, 300 * KB, 1.5, |r, i| format!("Microsoft.VisualStudio.{}{i}.dll", r.pick(WORDS)));
    }
    let vcl = g.path(vs, &["VC", "Tools", "MSVC", "14.48.35211", "lib", "x64"]);
    g.many(&mut r, vcl, 600, 3 * MB, 1.4, |r, i| format!("{}{i}.lib", r.pick(WORDS).to_lowercase()));
    let vci = g.path(vs, &["VC", "Tools", "MSVC", "14.48.35211", "include"]);
    g.many(&mut r, vci, 4000, 30 * KB, 1.2, |r, i| format!("{}{i}.h", r.pick(WORDS).to_lowercase()));

    let nvp = g.path(pfiles, &["NVIDIA Corporation", "NVIDIA app"]);
    g.many(&mut r, nvp, 300, 6 * MB, 1.4, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["dll", "exe", "pak", "dat"])));
    let off = g.path(pfiles, &["Microsoft Office", "root", "Office16"]);
    g.many(&mut r, off, 900, 3 * MB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_uppercase(), r.pick(&["DLL", "dll", "EXE"])));
    let chrome = g.path(pfiles, &["Google", "Chrome", "Application", "141.0.7390.123"]);
    g.file(chrome, "chrome.dll", 262 * MB);
    g.file(chrome, "resources.pak", 21 * MB);
    let loc = g.dir(chrome, "Locales");
    g.many(&mut r, loc, 55, 800 * KB, 0.3, |r, _| format!("{}.pak", r.pick(&["es", "en-US", "fr", "de", "it", "pt-BR", "ja", "ko", "zh-CN", "ru"])));
    let bl = g.path(pfiles, &["Blender Foundation", "Blender 4.5"]);
    g.file(bl, "blender.exe", 180 * MB);
    let blp = g.path(bl, &["4.5", "python", "lib"]);
    for i in 0..40 {
        let d = g.dir(blp, &format!("{}{i}", r.pick(WORDS).to_lowercase()));
        g.many(&mut r, d, 230, 6 * KB, 1.4, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["py", "py", "pyc", "pyd"])));
    }
    let git = g.path(pfiles, &["Git", "usr", "bin"]);
    g.many(&mut r, git, 300, 400 * KB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["exe", "dll"])));
    let node = g.path(pfiles, &["nodejs"]);
    g.file(node, "node.exe", 92 * MB);
    let nnm = g.path(node, &["node_modules", "npm", "node_modules"]);
    for i in 0..220 {
        let d = g.dir(nnm, &format!("{}-{i}", r.pick(WORDS).to_lowercase()));
        let n = r.range(4, 40);
        g.many(&mut r, d, n, 4 * KB, 1.3, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(CODE_EXTS)));
    }
    let wapps = g.dir_f(pfiles, "WindowsApps", flags::HIDDEN);
    for (app, n, med) in [
        ("Microsoft.MinecraftUWP_1.21.11201.0_x64__8wekyb3d8bbwe", 1400, 900 * KB),
        ("Microsoft.WindowsTerminal_1.23.12811.0_x64__8wekyb3d8bbwe", 300, 200 * KB),
        ("Microsoft.GamingApp_2610.1001.29.0_x64__8wekyb3d8bbwe", 600, 300 * KB),
        ("Clipchamp.Clipchamp_4.4.10000.0_x64__yxz26nhyzhsrt", 900, 400 * KB),
        ("MSTeams_26292.1.3922.4130_x64__8wekyb3d8bbwe", 700, 600 * KB),
    ] {
        let d = g.dir(wapps, app);
        g.many(&mut r, d, n, med, 1.8, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["dll", "png", "pri", "js", "pak", "zip", "json"])));
    }

    // ---------- Program Files (x86): Steam ----------
    let pf86 = g.dir(root, "Program Files (x86)");
    let steam = g.dir(pf86, "Steam");
    g.file(steam, "steam.exe", 4 * MB);
    g.file(steam, "steamui.dll", 380 * MB);
    let spk = g.dir(steam, "package");
    g.many(&mut r, spk, 40, 9 * MB, 1.3, |r, i| format!("{}_{i}.zip", r.pick(&["bins", "webkit", "public", "steamui"])));
    let common = g.path(steam, &["steamapps", "common"]);
    let cp = g.dir(common, "Cyberpunk 2077");
    let cpa = g.path(cp, &["archive", "pc", "content"]);
    g.many(&mut r, cpa, 30, 1_800 * MB, 0.8, |r, i| format!("basegame_{i}_{}.archive", r.pick(GAME_WORDS)));
    let ep1 = g.path(cp, &["archive", "pc", "ep1"]);
    g.many(&mut r, ep1, 12, 1_500 * MB, 0.6, |r, i| format!("ep1_{i}_{}.archive", r.pick(GAME_WORDS)));
    let cpb = g.path(cp, &["bin", "x64"]);
    g.file(cpb, "Cyberpunk2077.exe", 62 * MB);
    g.many(&mut r, cpb, 80, 4 * MB, 1.5, |r, i| format!("{}{i}.dll", r.pick(WORDS)));
    let bg3 = g.dir(common, "Baldurs Gate 3");
    let bgd = g.dir(bg3, "Data");
    g.file(bgd, "Gustav.pak", 32 * GB);
    g.file(bgd, "Shared.pak", 1_100 * MB);
    for i in 1..=4 {
        let s = r.size(7 * GB, 0.2);
        g.file(bgd, &format!("Textures_{i}.pak"), s);
    }
    for i in 1..=9 {
        let s = r.size(5 * GB, 0.35);
        g.file(bgd, &format!("VirtualTextures_{i}.pak"), s);
    }
    let bgv = g.dir(bgd, "Video");
    g.many(&mut r, bgv, 60, 120 * MB, 0.8, |r, i| format!("{}_{i}.bk2", r.pick(&["Intro", "Cinematic", "Ending", "Camp", "Act1", "Act2", "Act3"])));
    let bgb = g.dir(bg3, "bin");
    g.many(&mut r, bgb, 70, 7 * MB, 1.4, |r, i| format!("{}{i}.dll", r.pick(WORDS)));
    g.file(bgb, "bg3_dx11.exe", 88 * MB);
    let cs = g.path(common, &["Counter-Strike Global Offensive", "game", "csgo"]);
    for i in 0..216 {
        let s = r.size(100 * MB, 0.08);
        g.file(cs, &format!("pak01_{i:03}.vpk"), s);
    }
    g.file(cs, "pak01_dir.vpk", 2 * MB);
    let maps = g.dir(cs, "maps");
    g.many(&mut r, maps, 40, 180 * MB, 0.6, |r, i| format!("de_{}{i}.vpk", r.pick(&["dust", "mirage", "inferno", "nuke", "ancient", "anubis", "vertigo", "train"])));
    let pan = g.path(cs, &["panorama", "videos"]);
    g.many(&mut r, pan, 80, 30 * MB, 0.8, |r, i| format!("{}{i}.webm", r.pick(&["map_bg", "intro", "agent", "case"])));
    let er = g.path(common, &["ELDEN RING", "Game"]);
    for (n, s) in [("Data0.bdt", 20 * GB), ("Data1.bdt", 4_300 * MB), ("Data2.bdt", 9_100 * MB), ("Data3.bdt", 6_200 * MB), ("Data0.bhd", 2 * MB)] {
        g.file(er, n, s);
    }
    let erm = g.dir(er, "movie");
    g.many(&mut r, erm, 30, 150 * MB, 0.6, |_, i| format!("m{:02}_{:02}.bk2", i / 4, i % 4));
    let hk = g.path(common, &["Hollow Knight", "hollow_knight_Data"]);
    g.many(&mut r, hk, 40, 60 * MB, 1.2, |_, i| format!("sharedassets{i}.assets"));
    g.file(hk, "resources.assets", 900 * MB);
    let sv = g.path(common, &["Stardew Valley", "Content"]);
    g.many(&mut r, sv, 3000, 40 * KB, 1.3, |r, i| format!("{}{i}.xnb", r.pick(GAME_WORDS)));
    let shc = g.path(steam, &["steamapps", "shadercache"]);
    for app in ["1091500", "1086940", "730", "1245620", "367520"] {
        let d = g.path(shc, &[app, "fozpipelinesv6"]);
        let s = r.size(900 * MB, 0.6);
        g.file(d, "steamapprun_pipeline_cache.foz", s);
        g.many(&mut r, d, 8, 20 * MB, 1.0, |_, i| format!("steam_pipeline_cache.{i}.foz"));
    }
    let ws = g.path(steam, &["steamapps", "workshop", "content", "730"]);
    for i in 0..60 {
        let d = g.dir(ws, &format!("{}", 3_100_000_000u64 + i * 7919));
        let s = r.size(90 * MB, 1.0);
        g.file(d, "map.vpk", s);
    }
    let edge = g.path(pf86, &["Microsoft", "Edge", "Application", "141.0.3537.71"]);
    g.file(edge, "msedge.dll", 268 * MB);
    g.many(&mut r, edge, 120, 3 * MB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["dll", "pak", "exe", "dat"])));
    let wk = g.path(pf86, &["Windows Kits", "10"]);
    let wkl = g.dir(wk, "Lib");
    g.many(&mut r, wkl, 2000, 900 * KB, 1.5, |r, i| format!("{}{i}.lib", r.pick(WORDS).to_lowercase()));
    let wki = g.dir(wk, "Include");
    g.many(&mut r, wki, 5000, 40 * KB, 1.3, |r, i| format!("{}{i}.h", r.pick(WORDS).to_lowercase()));

    // juegos de Xbox y de Epic
    let xg = g.dir(root, "XboxGames");
    let fh = g.path(xg, &["Forza Horizon 5", "Content", "media"]);
    g.many(&mut r, fh, 42, 2_200 * MB, 0.6, |r, i| format!("{}_{i}.zip", r.pick(&["Stripped", "Cars", "Tracks", "Audio", "UI", "Base"])));
    let stf = g.path(xg, &["Starfield", "Content", "Data"]);
    g.many(&mut r, stf, 24, 2_800 * MB, 0.6, |r, i| format!("Starfield - {}{i}.ba2", r.pick(&["Textures", "Meshes", "Voices", "Animations", "Interface", "Terrain"])));
    let fn_ = g.path(root, &["Juegos", "Epic Games", "Fortnite", "FortniteGame", "Content", "Paks"]);
    for i in 0..40 {
        let s = r.size(800 * MB, 1.0);
        g.file(fn_, &format!("pakchunk{i}-WindowsClient.ucas"), s);
        g.file(fn_, &format!("pakchunk{i}-WindowsClient.utoc"), s / 300);
        g.file(fn_, &format!("pakchunk{i}-WindowsClient.pak"), 64 * KB);
        g.file(fn_, &format!("pakchunk{i}-WindowsClient.sig"), 40 * KB);
    }

    // ---------- ProgramData ----------
    let pd = g.dir_f(root, "ProgramData", flags::HIDDEN);
    let def = g.path(pd, &["Microsoft", "Windows Defender", "Definition Updates", "{5E1A7F2C-0B4E-4C1E-9D8E-0A3C7B1D2E4F}"]);
    for (n, s) in [("mpavbase.vdm", 92 * MB), ("mpasbase.vdm", 70 * MB), ("mpengine.dll", 24 * MB), ("mpavdlta.vdm", 6 * MB)] {
        g.file(def, n, s);
    }
    let pc = g.dir(pd, "Package Cache");
    for _ in 0..140 {
        let d = g.dir(pc, &format!("{{{}}}v14.44.35211", r.hex(8).to_uppercase()));
        let n = r.range(1, 3);
        g.many(&mut r, d, n, 18 * MB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["msi", "cab", "exe"])));
    }
    let nvd = g.path(pd, &["NVIDIA Corporation", "Downloader"]);
    for i in 0..6 {
        let d = g.dir(nvd, &r.hex(32));
        g.file(d, &format!("596.{i}2-desktop-win10-win11-64bit-international-dch-whql.exe"), r.size(900 * MB, 0.1));
    }
    let srch = g.path(pd, &["Microsoft", "Search", "Data", "Applications", "Windows"]);
    g.file(srch, "Windows.edb", 1_260 * MB);
    let wer = g.path(pd, &["Microsoft", "Windows", "WER", "ReportArchive"]);
    for i in 0..60 {
        let d = g.dir(wer, &format!("AppCrash_{}_{}", r.pick(WORDS), i));
        g.file(d, "Report.wer", 20 * KB);
        if r.chance(0.3) {
            g.file(d, &format!("memory_{i}.hdmp"), r.size(250 * MB, 0.8));
        }
    }

    // ---------- Users ----------
    let users = g.dir(root, "Users");
    let u = g.dir(users, "usuario");
    let vid = g.dir(u, "Videos");
    let cap = g.dir(vid, "Capturas");
    g.many(&mut r, cap, 30, 2_500 * MB, 0.9, |r, _| format!("2026-{:02}-{:02} {:02}-{:02}-{:02}.mkv", r.range(1, 10), r.range(1, 28), r.range(16, 23), r.below(60), r.below(60)));
    let proj = g.path(vid, &["Proyectos", "Corto 2025"]);
    g.file(proj, "Corto 2025.prproj", 48 * MB);
    g.many(&mut r, proj, 6, 5 * GB, 0.4, |_, i| format!("Render final v{}.mp4", i + 1));
    let media = g.dir(proj, "Media");
    g.many(&mut r, media, 200, 300 * MB, 1.2, |r, i| format!("C{:04}.{}", i + 1, r.pick(&["MP4", "MOV", "mp4"])));
    let aud = g.dir(proj, "Audio");
    g.many(&mut r, aud, 40, 60 * MB, 0.8, |r, i| format!("{} {i}.wav", r.pick(&["Ambiente", "Voz", "Música", "Efecto"])));
    let pel = g.dir(vid, "Películas");
    g.many(&mut r, pel, 12, 6 * GB, 0.5, |r, i| format!("{} ({}) 2160p.mkv", r.pick(&["La Montaña", "El Faro", "Niebla", "Mareas", "Ciudad Dormida", "Los Viajeros"]), 1990 + i * 3));
    let clips = g.dir(vid, "Clips");
    g.many(&mut r, clips, 80, 80 * MB, 1.0, |_, i| format!("Clip {i}.mp4"));

    let pic = g.dir(u, "Pictures");
    let cam = g.dir(pic, "Cámara");
    for y in 2019..=2026 {
        let d = g.dir(cam, &y.to_string());
        g.many(&mut r, d, 1100, 3_500 * KB, 0.5, |r, i| match r.below(20) {
            0..=14 => format!("IMG_{y}{:04}.jpg", i),
            15..=17 => format!("IMG_{y}{:04}.HEIC", i),
            _ => format!("VID_{y}{:04}.mp4", i),
        });
    }
    let raw = g.dir(pic, "RAW");
    for s in ["Boda Marta y Javier", "Asturias 2025", "Retratos estudio", "Concierto"] {
        let d = g.dir(raw, s);
        g.many(&mut r, d, 380, 28 * MB, 0.25, |_, i| format!("_MG_{:04}.CR3", 1000 + i));
        g.many(&mut r, d, 380, 8 * KB, 0.3, |_, i| format!("_MG_{:04}.xmp", 1000 + i));
    }
    let shots = g.dir(pic, "Screenshots");
    g.many(&mut r, shots, 2500, 1_600 * KB, 0.6, |_, i| format!("Captura de pantalla {i}.png"));
    let lr = g.dir(pic, "Lightroom");
    g.file(lr, "Lightroom Catalog.lrcat", 2_100 * MB);
    let lrp = g.dir(lr, "Lightroom Catalog Previews.lrdata");
    g.many(&mut r, lrp, 6000, 400 * KB, 0.8, |r, _| format!("{}.lrprev", r.hex(32)));

    let mus = g.dir(u, "Music");
    for i in 0..300 {
        let an = format!("{} {} {i}", r.pick(PERSON), r.pick(&["Trío", "y los Faros", "Quartet", "Band", "Ensemble", "Orquesta"]));
        let a = g.dir(mus, &an);
        let n = r.range(8, 16);
        let flac = r.chance(0.18);
        g.many(&mut r, a, n, if flac { 28 * MB } else { 6 * MB }, 0.3, |r, j| format!("{:02} - {}.{}", j + 1, r.pick(WORDS), if flac { "flac" } else { "mp3" }));
        g.file(a, "cover.jpg", 400 * KB);
    }

    let docs = g.dir(u, "Documents");
    let fac = g.dir(docs, "Facturas");
    g.many(&mut r, fac, 600, 250 * KB, 0.8, |_, i| format!("Factura {:04}.pdf", i));
    let trab = g.dir(docs, "Trabajo");
    g.many(&mut r, trab, 1200, 600 * KB, 1.4, |r, i| format!("{} {i}.{}", r.pick(&["Informe", "Presupuesto", "Presentación", "Acta", "Plan"]), r.pick(&["docx", "xlsx", "pptx", "pdf"])));
    let up = g.path(docs, &["Unreal Projects", "MiJuego"]);
    g.file(up, "MiJuego.uproject", 3 * KB);
    let upc = g.dir(up, "Content");
    for w in GAME_WORDS {
        let d = g.dir(upc, w);
        g.many(&mut r, d, 500, 200 * KB, 2.0, |r, i| format!("{}_{}{i}.{}", r.pick(&["T", "SM", "M", "BP", "A"]), r.pick(WORDS), if r.chance(0.01) { "umap" } else { "uasset" }));
    }
    let upi = g.path(up, &["Intermediate", "Build", "Win64", "x64", "UnrealEditor", "Development"]);
    g.many(&mut r, upi, 3000, 900 * KB, 1.5, |r, i| format!("Module.{}{i}.cpp.obj", r.pick(WORDS)));
    g.many(&mut r, upi, 8, 300 * MB, 0.4, |r, i| format!("SharedPCH.{}{i}.h.pch", r.pick(WORDS)));
    let upl = g.path(up, &["Saved", "Logs"]);
    g.many(&mut r, upl, 200, 2 * MB, 1.2, |_, i| format!("MiJuego-backup-2026.10.{:02}-{i}.log", 1 + i % 30));
    let upd = g.dir(up, "DerivedDataCache");
    g.many(&mut r, upd, 20000, 30 * KB, 1.6, |r, _| format!("{}.udd", r.hex(24)));
    let upb = g.path(up, &["Binaries", "Win64"]);
    g.file(upb, "UnrealEditor-MiJuego.dll", 40 * MB);
    g.file(upb, "UnrealEditor-MiJuego.pdb", 1_800 * MB);
    let gd = g.path(docs, &["Proyectos Godot", "Isla"]);
    g.many(&mut r, gd, 600, 30 * KB, 1.6, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["tscn", "gd", "import", "png", "tres", "ogg"])));
    let mg = g.path(docs, &["My Games", "Saves"]);
    g.many(&mut r, mg, 40, 3 * MB, 1.0, |_, i| format!("partida{i}.sav"));

    let dl = g.dir(u, "Downloads");
    g.file(dl, "Win11_25H2_Spanish_x64.iso", 6_900 * MB);
    g.file(dl, "ubuntu-26.04-desktop-amd64.iso", 6_200 * MB);
    g.many(&mut r, dl, 120, 80 * MB, 1.3, |r, i| format!("{}Setup-{i}.exe", r.pick(&["Steam", "Discord", "Spotify", "Obs", "Blender", "Vlc", "Python", "Git", "Zoom", "Nvidia"])));
    g.many(&mut r, dl, 150, 40 * MB, 1.6, |r, i| format!("{} {i}.{}", r.pick(&["Fotos", "Proyecto", "Assets", "Backup", "Plantillas"]), r.pick(&["zip", "zip", "7z", "rar"])));
    g.many(&mut r, dl, 300, 2 * MB, 1.2, |r, i| format!("{} {i}.pdf", r.pick(&["Manual", "Factura", "Billete", "Contrato", "Artículo"])));
    g.many(&mut r, dl, 20, 300 * MB, 1.0, |_, i| format!("video ({i}).mp4"));
    let tg = g.path(dl, &["Telegram Desktop"]);
    g.many(&mut r, tg, 400, 2 * MB, 1.4, |r, i| format!("{}_{i}.{}", r.pick(&["photo", "video", "file"]), r.pick(&["jpg", "jpg", "mp4", "pdf"])));
    let desk = g.dir(u, "Desktop");
    g.many(&mut r, desk, 30, 2 * KB, 0.3, |r, _| format!("{}.lnk", r.pick(WORDS)));
    g.file(desk, "Proyecto final.zip", 1_200 * MB);
    let od = g.dir(u, "OneDrive");
    g.many(&mut r, od, 300, 4 * MB, 1.4, |r, i| format!("{} {i}.{}", r.pick(&["Documento", "Foto", "Hoja"]), r.pick(&["docx", "jpg", "xlsx"])));
    for i in 0..600 {
        let s = r.size(5 * MB, 1.5);
        g.file_f(od, &format!("En la nube {i}.jpg"), s, flags::OFFLINE);
    }

    // repositorios: node_modules y target/ a montones
    let repos = g.path(u, &["source", "repos"]);
    for (name, kind) in [
        ("desvan", 'r'),
        ("web-portfolio", 'n'),
        ("tienda-online", 'n'),
        ("motor-fisica", 'r'),
        ("bot-discord", 'n'),
        ("api-facturas", 'c'),
        ("app-movil", 'n'),
        ("compilador-juguete", 'r'),
        ("panel-admin", 'n'),
        ("scraper", 'p'),
        ("juego-plataformas", 'c'),
        ("dotfiles", 'p'),
    ]
    {
        let p = g.dir(repos, name);
        let srcd = g.dir(p, "src");
        let ext = match kind {
            'r' => "rs",
            'n' => "ts",
            'c' => "cs",
            _ => "py",
        };
        let n = r.range(40, 300);
        g.many(&mut r, srcd, n, 8 * KB, 1.0, |r, i| format!("{}{i}.{ext}", r.pick(WORDS).to_lowercase()));
        let pack = g.path(p, &[".git", "objects", "pack"]);
        let s = r.size(120 * MB, 1.2);
        g.file(pack, &format!("pack-{}.pack", r.hex(40)), s);
        g.file(pack, &format!("pack-{}.idx", r.hex(40)), s / 40);
        match kind {
            'n' => {
                let nm = g.dir(p, "node_modules");
                let pkgs = r.range(250, 900);
                for k in 0..pkgs {
                    let d = g.dir(nm, &format!("{}-{}{k}", r.pick(WORDS).to_lowercase(), r.pick(&["js", "core", "utils", "plugin", "loader", "kit"])));
                    let n = r.range(3, 40);
                    g.many(&mut r, d, n, 5 * KB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(CODE_EXTS)));
                }
                let swc = g.path(nm, &["@next", "swc-win32-x64-msvc"]);
                g.file(swc, "next-swc.win32-x64-msvc.node", 134 * MB);
                let esb = g.path(nm, &["@esbuild", "win32-x64"]);
                g.file(esb, "esbuild.exe", 10 * MB);
            }
            'r' => {
                let deps = g.path(p, &["target", "debug", "deps"]);
                g.many(&mut r, deps, 450, 2 * MB, 1.4, |r, i| format!("lib{}-{}.{}", r.pick(WORDS).to_lowercase(), r.hex(16), if i % 3 == 0 { "rmeta" } else { "rlib" }));
                g.many(&mut r, deps, 450, 2 * KB, 0.8, |r, _| format!("{}-{}.d", r.pick(WORDS).to_lowercase(), r.hex(16)));
                g.many(&mut r, deps, 20, 60 * MB, 0.8, |_, i| format!("{name}-{i}.pdb"));
                let inc = g.path(p, &["target", "debug", "incremental"]);
                for _ in 0..30 {
                    let dn = format!("{name}-{}", r.hex(13));
                    let d = g.dir(inc, &dn);
                    g.many(&mut r, d, 100, 200 * KB, 1.6, |r, i| format!("{}{i}.o", r.hex(10)));
                }
                let rel = g.path(p, &["target", "release"]);
                g.file(rel, &format!("{name}.exe"), 12 * MB);
            }
            'c' => {
                let bin = g.path(p, &["bin", "Debug", "net9.0"]);
                g.many(&mut r, bin, 200, 400 * KB, 1.4, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["dll", "pdb", "dll"])));
                let obj = g.path(p, &["obj", "Debug"]);
                g.many(&mut r, obj, 300, 200 * KB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["dll", "cache", "json"])));
            }
            _ => {
                let venv = g.path(p, &[".venv", "Lib", "site-packages"]);
                for k in 0..60 {
                    let d = g.dir(venv, &format!("{}{k}", r.pick(WORDS).to_lowercase()));
                    let n = r.range(5, 60);
                    g.many(&mut r, d, n, 10 * KB, 1.5, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["py", "pyc", "pyd"])));
                }
            }
        }
    }

    let cargo = g.path(u, &[".cargo", "registry"]);
    let cc = g.path(cargo, &["cache", "index.crates.io-1949cf8c6b5b557f"]);
    g.many(&mut r, cc, 2500, 80 * KB, 1.4, |r, i| format!("{}-0.{}.{}.crate", r.pick(WORDS).to_lowercase(), i % 30, i % 7));
    let csrc = g.path(cargo, &["src", "index.crates.io-1949cf8c6b5b557f"]);
    for i in 0..1200 {
        let d = g.dir(csrc, &format!("{}-0.{}.{}", r.pick(WORDS).to_lowercase(), i % 30, i % 7));
        let n = r.range(5, 80);
        g.many(&mut r, d, n, 9 * KB, 1.3, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["rs", "rs", "rs", "toml", "md"])));
    }
    let rup = g.path(u, &[".rustup", "toolchains", "stable-x86_64-pc-windows-msvc"]);
    let rl = g.path(rup, &["lib", "rustlib", "x86_64-pc-windows-msvc", "lib"]);
    g.many(&mut r, rl, 200, 4 * MB, 1.5, |r, i| format!("lib{}-{}.rlib", r.pick(WORDS).to_lowercase(), i));
    let rdoc = g.path(rup, &["share", "doc", "rust", "html"]);
    for i in 0..60 {
        let d = g.dir(rdoc, &format!("{}{i}", r.pick(WORDS).to_lowercase()));
        g.many(&mut r, d, 500, 12 * KB, 1.0, |r, i| format!("{}{i}.html", r.pick(WORDS).to_lowercase()));
    }
    let vsx = g.path(u, &[".vscode", "extensions"]);
    for i in 0..80 {
        let d = g.dir(vsx, &format!("{}.{}-1.{i}.0", r.pick(PERSON).to_lowercase(), r.pick(WORDS).to_lowercase()));
        g.many(&mut r, d, 180, 12 * KB, 1.8, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(CODE_EXTS)));
    }
    let lms = g.path(u, &[".lmstudio", "models", "lmstudio-community"]);
    for (m, s) in [("Qwen3-14B-GGUF", 9 * GB), ("gemma-3-12b-it-GGUF", 7_600 * MB), ("Llama-4-Scout-GGUF", 22 * GB), ("Mistral-Small-3.2-GGUF", 14 * GB), ("phi-4-GGUF", 8_400 * MB)] {
        let d = g.dir(lms, m);
        g.file(d, &format!("{}-Q4_K_M.gguf", m.trim_end_matches("-GGUF")), s);
    }
    let comfy = g.dir(u, "ComfyUI");
    let ck = g.path(comfy, &["models", "checkpoints"]);
    g.many(&mut r, ck, 6, 6 * GB, 0.3, |r, i| format!("{}_{i}.safetensors", r.pick(&["sdxl_base", "flux1-dev", "juggernaut", "realvis"])));
    let lo = g.path(comfy, &["models", "loras"]);
    g.many(&mut r, lo, 30, 150 * MB, 0.8, |r, i| format!("{}_{i}.safetensors", r.pick(WORDS).to_lowercase()));
    let co = g.dir(comfy, "output");
    g.many(&mut r, co, 2000, 1_500 * KB, 0.4, |_, i| format!("ComfyUI_{:05}_.png", i));
    let oll = g.path(u, &[".ollama", "models", "blobs"]);
    for s in [4_700 * MB, 19 * GB, 2_000 * MB, 8_900 * MB] {
        let h = r.hex(64);
        g.file(oll, &format!("sha256-{h}"), s);
    }

    let appl = g.path(u, &["AppData", "Local"]);
    let tmpu = g.dir(appl, "Temp");
    g.many(&mut r, tmpu, 4000, 60 * KB, 2.2, |r, _| format!("{}.{}", r.hex(8), r.pick(&["tmp", "tmp", "log", "dat"])));
    let dk = g.path(appl, &["Docker", "wsl", "disk"]);
    g.file(dk, "docker_data.vhdx", 58 * GB);
    let wsl = g.path(appl, &["Packages", "CanonicalGroupLimited.Ubuntu24.04LTS_79rhkp1fndgsc", "LocalState"]);
    g.file(wsl, "ext4.vhdx", 31 * GB);
    let teams = g.path(appl, &["Packages", "MSTeams_8wekyb3d8bbwe", "LocalCache", "Microsoft", "MSTeams", "EBWebView", "Default", "Cache", "Cache_Data"]);
    g.many(&mut r, teams, 5000, 50 * KB, 1.8, |_, i| format!("f_{:06x}", i));
    let wa = g.path(appl, &["Packages", "5319275A.WhatsAppDesktop_cv1g1gvanyjgm", "LocalState", "shared", "transfers"]);
    g.many(&mut r, wa, 1500, 3 * MB, 1.4, |r, i| format!("{}{i}.{}", r.hex(10), r.pick(&["jpg", "jpg", "mp4", "opus", "pdf"])));
    for (b, n) in [("Microsoft\\Edge", 12000), ("Google\\Chrome", 18000)] {
        let parts: Vec<&str> = b.split('\\').chain(["User Data", "Default", "Cache", "Cache_Data"]).collect();
        let c = g.path(appl, &parts);
        g.many(&mut r, c, n, 30 * KB, 1.9, |_, i| format!("f_{:06x}", i));
    }
    let dx = g.path(appl, &["NVIDIA", "DXCache"]);
    g.many(&mut r, dx, 2500, 200 * KB, 1.6, |r, _| format!("{}.nvph", r.hex(16)));
    let cdmp = g.dir(appl, "CrashDumps");
    g.many(&mut r, cdmp, 25, 300 * MB, 1.0, |r, i| format!("{}.exe.{}.dmp", r.pick(&["UnrealEditor", "Cyberpunk2077", "chrome", "obs64", "Discord"]), 10_000 + i));
    let py = g.path(appl, &["Programs", "Python", "Python313", "Lib"]);
    for i in 0..50 {
        let d = g.dir(py, &format!("{}{i}", r.pick(WORDS).to_lowercase()));
        g.many(&mut r, d, 140, 14 * KB, 1.4, |r, i| format!("{}{i}.{}", r.pick(WORDS).to_lowercase(), r.pick(&["py", "py", "pyc"])));
    }
    let npmc = g.path(appl, &["npm-cache", "_cacache", "content-v2", "sha512"]);
    for i in 0..256 {
        let d = g.dir(npmc, &format!("{:02x}", i));
        g.many(&mut r, d, 80, 20 * KB, 1.7, |r, _| r.hex(126));
    }
    let ued = g.path(appl, &["UnrealEngine", "Common", "DerivedDataCache"]);
    g.many(&mut r, ued, 30000, 50 * KB, 1.8, |r, _| format!("{}.udd", r.hex(24)));
    let thumbs = g.path(appl, &["Microsoft", "Windows", "Explorer"]);
    for n in ["thumbcache_256.db", "thumbcache_1280.db", "thumbcache_idx.db", "iconcache_32.db"] {
        g.file(thumbs, n, r.size(90 * MB, 1.0));
    }
    let spot = g.path(appl, &["Spotify", "Data"]);
    for i in 0..256 {
        let d = g.dir(spot, &format!("{:02x}", i));
        g.many(&mut r, d, 35, 600 * KB, 1.2, |r, _| format!("{}.file", r.hex(32)));
    }

    let appr = g.path(u, &["AppData", "Roaming"]);
    let dc = g.path(appr, &["discord", "Cache", "Cache_Data"]);
    g.many(&mut r, dc, 5000, 40 * KB, 1.8, |_, i| format!("f_{:06x}", i));
    let rec = g.path(appr, &["Microsoft", "Windows", "Recent"]);
    g.many(&mut r, rec, 800, 2 * KB, 0.4, |r, i| format!("{} {i}.lnk", r.pick(WORDS)));
    let amc = g.path(appr, &["Adobe", "Common", "Media Cache Files"]);
    g.many(&mut r, amc, 900, 7 * MB, 1.5, |r, i| format!("C{:04} {}.{}", i, r.hex(8), r.pick(&["cfa", "pek"])));
    let tgd = g.path(appr, &["Telegram Desktop", "tdata", "user_data", "media_cache"]);
    g.many(&mut r, tgd, 3000, 300 * KB, 1.6, |r, _| r.hex(16));

    let def_u = g.dir(users, "Default");
    g.file_f(def_u, "NTUSER.DAT", 2 * MB, sys);
    let publ = g.path(users, &["Public", "Documents"]);
    g.many(&mut r, publ, 20, 200 * KB, 1.0, |_, i| format!("doc{i}.pdf"));

    // papelera, instantáneas y restos varios
    let rb = g.path(root, &["$Recycle.Bin", "S-1-5-21-3623811015-3361044348-30300820-1001"]);
    g.many(&mut r, rb, 6, 900 * MB, 0.8, |r, _| format!("$R{}.mp4", r.hex(6).to_uppercase()));
    g.many(&mut r, rb, 10, 60 * MB, 1.2, |r, _| format!("$R{}.zip", r.hex(6).to_uppercase()));
    g.many(&mut r, rb, 16, 120, 0.2, |r, _| format!("$I{}", r.hex(6).to_uppercase()));
    let svi = g.dir_f(root, "System Volume Information", sys);
    for _ in 0..3 {
        let h = r.hex(8);
        let s = r.size(6 * GB, 0.3);
        g.file_f(svi, &format!("{{{h}-c176-4e48-b7ae-04046e6cc752}}{{3808876b-c176-4e48-b7ae-04046e6cc752}}"), s, sys);
    }
    g.file_f(svi, "Syscache.hve", 30 * MB, sys);
    let re = g.path(root, &["Recovery", "WindowsRE"]);
    g.file(re, "Winre.wim", 710 * MB);
    let esd = g.path(root, &["ESD", "Windows", "sources"]);
    g.file(esd, "install.esd", 4_600 * MB);
    let amd = g.path(root, &["AMD", "Chipset_Software"]);
    g.many(&mut r, amd, 40, 20 * MB, 1.2, |r, i| format!("{}{i}.{}", r.pick(WORDS), r.pick(&["exe", "msi", "cab"])));
    g.dir(root, "PerfLogs");
    let ine = g.path(root, &["inetpub", "wwwroot"]);
    g.file(ine, "iisstart.htm", 700);

    let names_bytes = g.b.names.len();
    let tree = g.b.build(root, "C:\\");
    (tree, names_bytes as u64)
}

/// Cuántos hijos válidos (no enlace duro, con tamaño) tiene cada carpeta: para los «restos».
fn valid_children(t: &Tree, n: NodeId) -> u64 {
    t.children(n).iter().filter(|&&c| t.flags[c as usize] & (flags::HARDLINK | flags::DELETED) == 0 && t.alloc[c as usize] > 0).count() as u64
}

/// Espacio por categoría bajo `n` (sin enlaces duros).
fn cat_totals(t: &Tree, cats: &[u8], n: NodeId) -> Vec<u64> {
    let mut tot = vec![0u64; categories::COUNT];
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        let f = t.flags[x as usize];
        if f & flags::HARDLINK != 0 {
            continue;
        }
        if f & flags::DIR != 0 {
            stack.extend_from_slice(t.children(x));
        } else {
            tot[categories::node_category(t, cats, x) as usize] += t.alloc[x as usize];
        }
    }
    tot
}

/// Escribe la vista de `node` y devuelve su layout.
fn dump(t: &Tree, cats: &[u8], node: NodeId, w: f32, h: f32, opts: &LayoutOpts, base: &Path, name: &str, parent_view: Option<NodeId>) -> Layout {
    let s = Instant::now();
    let l = layout::layout(t, node, w, h, opts);
    let ms = s.elapsed().as_secs_f64() * 1000.0;
    std::fs::write(base.with_file_name(format!("{name}.bin")), l.to_bytes(w, h)).expect("no se pudo escribir el .bin");

    // hijos dibujados por carpeta, para contar lo que queda en cada «resto»
    let mut drawn: HashMap<i64, u64> = HashMap::new();
    for i in 0..l.count {
        if l.flags(i) & rflags::REST == 0 {
            *drawn.entry(l.parent(i).map_or(-1, |p| p as i64)).or_default() += 1;
        }
    }
    let mut names = Vec::with_capacity(l.count);
    let mut sizes = Vec::with_capacity(l.count);
    let mut counts = Vec::with_capacity(l.count);
    for i in 0..l.count {
        let id = l.ids[i];
        let f = l.flags(i);
        if f & rflags::REST != 0 {
            let key = l.parent(i).map_or(-1, |p| p as i64);
            let n = valid_children(t, id).saturating_sub(drawn.get(&key).copied().unwrap_or(0));
            names.push(Value::from(format!("{} elementos pequeños", group_thousands(n))));
            // tamaño del resto: lo de la carpeta menos lo dibujado aparte
            let shown: u64 = (0..l.count).filter(|&j| l.parent(j).map_or(-1, |p| p as i64) == key && l.flags(j) & rflags::REST == 0).map(|j| t.alloc[l.ids[j] as usize]).sum();
            sizes.push(Value::from(t.alloc[id as usize].saturating_sub(shown)));
            counts.push(Value::from(n));
        } else {
            names.push(Value::from(t.name(id)));
            sizes.push(Value::from(t.alloc[id as usize]));
            counts.push(Value::from(if t.is_dir(id) { t.files[id as usize] } else { 1 }));
        }
    }
    let side = json!({
        "view": node,
        "parentView": parent_view,
        "path": t.path(node, "C:\\"),
        "name": if node == t.root { "C:\\".to_string() } else { t.name(node).to_string() },
        "total": t.alloc[node as usize],
        "size": t.size[node as usize],
        "files": t.files[node as usize],
        "dirs": t.dirs[node as usize],
        "count": l.count,
        "layoutMs": (ms * 100.0).round() / 100.0,
        "catTotals": cat_totals(t, cats, node),
        "names": names,
        "sizes": sizes,
        "counts": counts,
    });
    std::fs::write(base.with_file_name(format!("{name}.json")), serde_json::to_vec(&side).unwrap()).expect("no se pudo escribir el .json");
    println!("  {name}: {} rects en {ms:.1} ms  ({})", l.count, t.path(node, "C:\\"));
    l
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("uso: layout_dump <salida.bin> <ancho> <alto> [--views N] [--header PX] [--padding PX]");
        std::process::exit(2);
    }
    let out = PathBuf::from(&args[0]);
    let w: f32 = args[1].parse().expect("ancho");
    let h: f32 = args[2].parse().expect("alto");
    let flag = |name: &str, default: f32| -> f32 { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default) };
    let max_views = flag("--views", 16.0) as usize;
    let opts = LayoutOpts { header: flag("--header", 18.0), dir_padding: flag("--padding", 2.0), ..Default::default() };
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let stem = out.file_stem().unwrap().to_string_lossy().to_string();

    let s = Instant::now();
    let (t, _) = build_volume();
    println!("árbol sintético: {} nodos, {:.1} GB en disco, en {:?}", t.len(), t.alloc[t.root as usize] as f64 / GB as f64, s.elapsed());
    let cats = categories::ext_categories(&t);

    // vista raíz y subcarpetas grandes (para probar el zoom), en anchura
    let mut views: Vec<Value> = Vec::new();
    let mut queue: Vec<(NodeId, Option<NodeId>, u32)> = vec![(t.root, None, 0)];
    let mut done = 0;
    while !queue.is_empty() && done < max_views {
        let (node, parent, level) = queue.remove(0);
        let name = if node == t.root { stem.clone() } else { format!("{stem}.v{node}") };
        let l = dump(&t, &cats, node, w, h, &opts, &out, &name, parent);
        views.push(json!({ "id": node, "parent": parent, "file": name, "name": t.name(node), "path": t.path(node, "C:\\") }));
        done += 1;
        if level < 3 {
            // carpetas de primer nivel de esta vista que ocupan al menos un 3 %
            let mut kids: Vec<(f32, NodeId)> = (0..l.count)
                .filter(|&i| l.parent(i).is_none() && l.flags(i) & rflags::DIR != 0 && l.flags(i) & rflags::COLLAPSED == 0)
                .map(|i| {
                    let [_, _, rw, rh] = l.rect(i);
                    (rw * rh / (w * h), l.ids[i])
                })
                .filter(|&(a, _)| a >= 0.03)
                .collect();
            kids.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, k) in kids.into_iter().take(if level == 0 { 7 } else { 3 }) {
                queue.push((k, Some(node), level + 1));
            }
        }
    }
    let volume_total: u64 = 1_000 * GB;
    let index = json!({
        "root": t.root,
        "rootPath": "C:\\",
        "width": w,
        "height": h,
        "volume": { "total": volume_total, "used": t.alloc[t.root as usize], "label": "Windows", "filesystem": "NTFS" },
        "nodes": t.len(),
        "categories": categories::CATEGORIES.iter().map(|c| json!({ "id": c.0, "label": c.1 })).collect::<Vec<_>>(),
        "views": views,
    });
    std::fs::write(out.with_file_name(format!("{stem}.views.json")), serde_json::to_vec_pretty(&index).unwrap()).unwrap();
    println!("listo: {} vistas en {}", done, out.parent().map(|p| p.display().to_string()).unwrap_or_default());
}
