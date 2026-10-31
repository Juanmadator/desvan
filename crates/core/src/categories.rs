//! Categorías de archivo según la extensión (color del treemap, leyenda, estadísticas).
//!
//! La búsqueda usa una tabla hash abierta que se construye en tiempo de compilación:
//! la extensión (hasta 16 bytes ASCII) se empaqueta en minúsculas en un `u128`.
//! Una extensión repetida en dos listas es un error de compilación.

use crate::tree::{flags, NodeId, Tree};

pub const OTROS: u8 = 0;
pub const VIDEO: u8 = 1;
pub const IMAGEN: u8 = 2;
pub const AUDIO: u8 = 3;
pub const COMPRIMIDO: u8 = 4;
pub const DOCUMENTO: u8 = 5;
pub const CODIGO: u8 = 6;
pub const PROGRAMA: u8 = 7;
pub const SISTEMA: u8 = 8;
pub const JUEGO: u8 = 9;
/// Bases de datos, correo (pst/ost) y pesos de modelos de IA.
pub const DATOS: u8 = 10;
/// Diseño gráfico, 3D, proyectos de edición y fuentes tipográficas.
pub const DISENO: u8 = 11;

/// Número de categorías (los códigos van de 0 a `COUNT - 1`).
pub const COUNT: usize = 12;

/// (id estable, etiqueta en español), en el orden de los códigos.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("otros", "Otros"),
    ("video", "Vídeo"),
    ("imagen", "Imágenes"),
    ("audio", "Audio"),
    ("comprimido", "Comprimidos e imágenes de disco"),
    ("documento", "Documentos"),
    ("codigo", "Código y texto"),
    ("programa", "Programas y librerías"),
    ("sistema", "Sistema, temporales y logs"),
    ("juego", "Juegos y recursos"),
    ("datos", "Bases de datos y modelos de IA"),
    ("diseno", "Diseño, 3D y fuentes"),
];

const GROUPS: &[(u8, &[&str])] = &[
    (
        VIDEO,
        &[
            "mp4", "m4v", "mkv", "avi", "mov", "qt", "wmv", "flv", "f4v", "webm", "mpg", "mpeg", "mpe", "m1v", "m2v", "mpv", "m2ts", "mts",
            "m2t", "vob", "3gp", "3g2", "ogv", "rm", "rmvb", "asf", "divx", "xvid", "mxf", "dv", "h264", "h265", "264", "265", "hevc", "y4m",
            "yuv", "braw", "r3d", "insv", "lrv", "tod", "ivf", "nut", "wtv", "dvr-ms", "srt", "vtt", "ass", "ssa",
        ],
    ),
    (
        IMAGEN,
        &[
            "jpg", "jpeg", "jpe", "jfif", "pjpeg", "png", "apng", "gif", "bmp", "dib", "tif", "tiff", "webp", "heic", "heif", "avif", "jxl",
            "jp2", "j2k", "jpf", "jpx", "jxr", "wdp", "hdp", "ico", "cur", "icns", "svg", "svgz", "emf", "wmf", "raw", "cr2", "cr3", "crw",
            "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef", "srw", "x3f", "erf", "kdc", "3fr", "iiq", "mrw", "mos",
            "tga", "exr", "hdr", "dds", "ktx", "ktx2", "pcx", "ppm", "pgm", "pbm", "pnm", "xbm", "xpm", "qoi", "thm", "xmp",
        ],
    ),
    (
        AUDIO,
        &[
            "mp3", "mp2", "flac", "wav", "wave", "ogg", "oga", "opus", "m4a", "m4b", "m4p", "aac", "wma", "aiff", "aif", "aifc", "ape", "wv",
            "mka", "mid", "midi", "ac3", "eac3", "dts", "amr", "au", "snd", "caf", "ra", "dsf", "dff", "mpc", "spx", "xm", "it", "s3m",
            "voc", "w64", "rf64", "tta", "shn", "weba", "flp", "als", "ptx", "cpr", "aup", "aup3", "sf2", "sfz", "nki", "nkx", "ncw", "rx2",
            "rex",
        ],
    ),
    (
        COMPRIMIDO,
        &[
            "zip", "zipx", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "zst", "zstd", "tzst", "lz", "lzma", "tlz",
            "lz4", "lzh", "lha", "z", "cab", "arj", "ace", "cpio", "sit", "sitx", "xar", "iso", "img", "nrg", "mds", "cdi", "dmg", "vhd",
            "vhdx", "avhd", "avhdx", "vmdk", "vdi", "qcow", "qcow2", "hdd", "hds", "wim", "swm", "esd", "ffu", "ova", "tib", "tibx", "vbk",
            "vib", "gho", "sparseimage", "whl", "nupkg", "crate", "gem", "pack", "vsix", "xpi", "crx",
        ],
    ),
    (
        DOCUMENTO,
        &[
            "pdf", "doc", "docx", "docm", "dot", "dotx", "dotm", "odt", "ott", "rtf", "wpd", "wps", "pages", "xls", "xlsx", "xlsm", "xlsb",
            "xlt", "xltx", "xltm", "ods", "ots", "numbers", "csv", "tsv", "ppt", "pptx", "pptm", "pps", "ppsx", "pot", "potx", "odp", "otp",
            "key", "epub", "mobi", "azw", "azw3", "kfx", "fb2", "djvu", "djv", "xps", "oxps", "one", "onepkg", "onetoc2", "pub", "vsd",
            "vsdx", "mpp", "cbz", "cbr", "cb7", "chm", "ps", "msg", "eml", "emlx", "vcf", "ics", "hwp", "ofd", "gdoc", "gsheet", "gslides",
            "xmind", "enex",
        ],
    ),
    (
        CODIGO,
        &[
            "txt", "text", "md", "markdown", "mdx", "rst", "adoc", "asciidoc", "org", "tex", "bib", "sty", "ltx", "nfo", "rs", "toml", "lock",
            "c", "h", "cc", "cpp", "cxx", "c++", "hpp", "hh", "hxx", "h++", "inl", "ipp", "tcc", "m", "mm", "cs", "csx", "vb", "vba", "bas",
            "fs", "fsx", "fsi", "java", "kt", "kts", "scala", "sc", "groovy", "gradle", "go", "mod", "sum", "py", "pyi", "pyw", "pyx", "pxd",
            "rb", "erb", "rake", "gemspec", "php", "phtml", "pl", "pm", "pod", "lua", "luau", "r", "rmd", "jl", "dart", "swift", "zig", "nim",
            "v", "sv", "svh", "vhdl", "asm", "s", "nasm", "inc", "ex", "exs", "eex", "heex", "erl", "hrl", "hs", "lhs", "elm", "ml", "mli",
            "clj", "cljs", "cljc", "edn", "lisp", "lsp", "el", "scm", "ss", "rkt", "f", "f90", "f95", "f03", "for", "pas", "pp", "dpr", "lpr",
            "ada", "adb", "ads", "cob", "cbl", "js", "mjs", "cjs", "jsx", "ts", "cts", "tsx", "vue", "svelte", "astro", "html", "htm",
            "xhtml", "shtml", "css", "scss", "sass", "less", "styl", "pcss", "json", "jsonc", "json5", "ndjson", "jsonl", "geojson", "xml",
            "xsd", "xsl", "xslt", "dtd", "plist", "yaml", "yml", "ini", "cfg", "conf", "config", "properties", "env", "sh", "bash", "zsh",
            "fish", "ksh", "csh", "ps1", "psm1", "psd1", "ps1xml", "bat", "cmd", "vbs", "wsf", "ahk", "au3", "reg", "sql", "graphql", "gql",
            "proto", "thrift", "cmake", "mk", "mak", "ninja", "bazel", "bzl", "gn", "gni", "sln", "slnx", "csproj", "vbproj", "vcxproj",
            "fsproj", "sqlproj", "props", "targets", "filters", "nuspec", "ruleset", "tf", "tfvars", "hcl", "nix", "dhall", "glsl", "vert",
            "frag", "geom", "tesc", "tese", "comp", "hlsl", "hlsli", "fx", "fxh", "usf", "ush", "shader", "cginc", "compute", "wgsl", "metal",
            "cu", "cuh", "cl", "ipynb", "map", "diff", "patch", "d", "po", "resx", "xaml", "axaml", "razor", "cshtml", "vbhtml", "aspx",
            "ascx", "jsp", "hbs", "handlebars", "mustache", "ejs", "pug", "jade", "twig", "liquid", "j2", "jinja", "tmpl", "tpl", "cr", "pde",
            "ino", "gd", "tscn", "tres", "godot", "import", "uproject", "uplugin", "m4", "awk", "sed", "def", "rc", "idl", "meta", "asmdef",
        ],
    ),
    (
        PROGRAMA,
        &[
            "exe", "dll", "ocx", "cpl", "scr", "drv", "efi", "com", "msi", "msp", "msu", "msm", "mst", "msix", "msixbundle", "appx",
            "appxbundle", "appinstaller", "apk", "aab", "xapk", "apks", "ipa", "pkg", "deb", "rpm", "snap", "flatpak", "appimage", "so",
            "dylib", "a", "lib", "o", "obj", "pdb", "ilk", "exp", "idb", "ipdb", "iobj", "pch", "ipch", "gch", "rlib", "rmeta", "pyc", "pyo",
            "pyd", "class", "jar", "war", "ear", "dex", "node", "wasm", "bpl", "winmd", "tlb", "olb", "ax", "acm", "ime", "tsp", "xll", "ko",
            "vxd", "elf", "run", "vst", "vst3", "aaxplugin", "clap", "jnilib",
        ],
    ),
    (
        SISTEMA,
        &[
            "sys", "log", "log1", "log2", "tmp", "temp", "bak", "old", "orig", "dmp", "mdmp", "hdmp", "kdmp", "etl", "evtx", "evt", "blf",
            "regtrans-ms", "cache", "pf", "swp", "swo", "crdownload", "part", "partial", "download", "opdownload", "chk", "gid", "mui", "mun",
            "nls", "cat", "manifest", "mum", "cdf-ms", "lnk", "url", "inf", "pnf", "msc", "wer", "trace", "sdb", "sqm", "vdm", "vdmx", "pri",
            "jrs", "jfm", "lck", "pid", "tlog", "lastbuildstate", "nvph", "foz",
        ],
    ),
    (
        JUEGO,
        &[
            "uasset", "umap", "uexp", "ubulk", "uptnl", "upk", "udk", "utx", "uax", "umx", "unr", "pak", "ucas", "utoc", "sig", "vpk", "vtf",
            "vmt", "mdl", "vvd", "vtx", "phy", "bsp", "gma", "nav", "bsa", "ba2", "esp", "esm", "esl", "unity3d", "assets", "resource",
            "ress", "asset", "prefab", "unity", "anim", "controller", "forge", "rpf", "big", "wad", "pk3", "pk4", "pck", "xnb", "xwb", "xsb",
            "xgs", "fsb", "bnk", "wem", "ff", "iwd", "archive", "cpk", "awb", "acb", "usm", "bik", "bk2", "bdt", "bhd", "dcx", "arc", "gcf",
            "ncf", "acf", "sav", "save", "sl2", "ue4save", "nsp", "xci", "nds", "gba", "gbc", "gb", "nes", "sfc", "smc", "n64", "z64", "v64",
            "cia", "rvz", "wbfs", "gcm", "chd", "cso", "pbp", "mpq", "blp", "m2", "wmo", "nif", "kf", "hkx", "obb", "rpa", "rpyc", "rgss3a",
            "w3x", "w3m", "sc2map", "pk2", "package", "tfc", "cas", "rbxl", "rbxm",
        ],
    ),
    (
        DATOS,
        &[
            "db", "db3", "sqlite", "sqlite3", "s3db", "sl3", "sqlitedb", "mdb", "accdb", "accde", "mdf", "ndf", "ldf", "frm", "ibd", "myd",
            "myi", "dbf", "sdf", "edb", "kdbx", "realm", "ldb", "sst", "db-wal", "db-shm", "db-journal", "sqlite-wal", "sqlite-shm", "fdb",
            "gdb", "nsf", "pst", "ost", "mbox", "dbx", "wdb", "rdb", "aof", "bson", "parquet", "feather", "arrow", "orc", "avro", "h5", "hdf5",
            "hdf", "nc", "npy", "npz", "pkl", "pickle", "joblib", "dta", "sas7bdat", "rds", "rdata", "rda", "safetensors", "gguf", "ggml",
            "ckpt", "pt", "pth", "onnx", "tflite", "pb", "mlmodel", "keras", "caffemodel", "lmdb", "duckdb",
        ],
    ),
    (
        DISENO,
        &[
            "psd", "psb", "ai", "eps", "indd", "indt", "idml", "xd", "fig", "sketch", "afdesign", "afphoto", "afpub", "kra", "xcf", "clip",
            "sai", "sai2", "procreate", "ora", "cdr", "blend", "blend1", "fbx", "dae", "3ds", "max", "ma", "mb", "c4d", "lwo", "lws", "skp",
            "dwg", "dxf", "dgn", "rvt", "rfa", "ifc", "step", "stp", "iges", "igs", "stl", "3mf", "ply", "abc", "usd", "usda", "usdc", "usdz",
            "glb", "gltf", "vrm", "x3d", "ztl", "zpr", "spp", "sbsar", "sbs", "hip", "hipnc", "hiplc", "bgeo", "vdb", "mtl", "prproj", "aep",
            "aepx", "drp", "veg", "kdenlive", "ttf", "otf", "ttc", "otc", "woff", "woff2", "eot", "fon", "fnt", "pfb", "pfm", "afm", "odg",
            "cube", "aco", "ase", "abr", "atn",
        ],
    ),
];

const BITS: u32 = 12;
const SLOTS: usize = 1 << BITS;
const MASK: usize = SLOTS - 1;

struct Table {
    keys: [u128; SLOTS],
    cats: [u8; SLOTS],
}

static TABLE: Table = build();

/// Empaqueta la extensión en minúsculas; 0 si no cabe o no es ASCII imprimible.
const fn pack(b: &[u8]) -> u128 {
    if b.is_empty() || b.len() > 16 {
        return 0;
    }
    let mut k: u128 = 0;
    let mut i = 0;
    while i < b.len() {
        let mut c = b[i];
        if c <= b' ' || c >= 0x7f {
            return 0;
        }
        if c.is_ascii_uppercase() {
            c += 32;
        }
        k = (k << 8) | c as u128;
        i += 1;
    }
    k
}

const fn slot(k: u128) -> usize {
    let x = (k as u64) ^ ((k >> 64) as u64).rotate_left(29);
    (x.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - BITS)) as usize
}

const fn build() -> Table {
    let mut t = Table { keys: [0; SLOTS], cats: [0; SLOTS] };
    let mut g = 0;
    while g < GROUPS.len() {
        let cat = GROUPS[g].0;
        let list = GROUPS[g].1;
        let mut i = 0;
        while i < list.len() {
            let k = pack(list[i].as_bytes());
            assert!(k != 0, "extensión no válida en la tabla de categorías");
            let mut s = slot(k);
            loop {
                if t.keys[s] == 0 {
                    t.keys[s] = k;
                    t.cats[s] = cat;
                    break;
                }
                assert!(t.keys[s] != k, "extensión repetida en la tabla de categorías");
                s = (s + 1) & MASK;
            }
            i += 1;
        }
        g += 1;
    }
    t
}

/// Categoría de una extensión (sin punto o con él; da igual mayúsculas o minúsculas).
#[inline]
pub fn category(ext: &str) -> u8 {
    let b = ext.as_bytes();
    let b = if b.first() == Some(&b'.') { &b[1..] } else { b };
    let k = pack(b);
    if k == 0 {
        return OTROS;
    }
    let mut s = slot(k);
    loop {
        let kk = TABLE.keys[s];
        if kk == k {
            return TABLE.cats[s];
        }
        if kk == 0 {
            return OTROS;
        }
        s = (s + 1) & MASK;
    }
}

/// Id estable de una categoría ("video"…); "otros" si no existe.
pub fn id(cat: u8) -> &'static str {
    CATEGORIES.get(cat as usize).map_or(CATEGORIES[0].0, |c| c.0)
}

/// Etiqueta en español de una categoría.
pub fn label(cat: u8) -> &'static str {
    CATEGORIES.get(cat as usize).map_or(CATEGORIES[0].1, |c| c.1)
}

/// Categoría de cada extensión de la tabla del árbol (índice = id de extensión).
pub fn ext_categories(tree: &Tree) -> Vec<u8> {
    tree.exts.iter().map(|e| category(e)).collect()
}

/// Categoría de un archivo. Los metarchivos de NTFS ($MFT…) cuentan como sistema.
#[inline]
pub fn node_category(tree: &Tree, ext_cats: &[u8], n: NodeId) -> u8 {
    if tree.flags[n as usize] & flags::METAFILE != 0 {
        return SISTEMA;
    }
    ext_cats.get(tree.ext[n as usize] as usize).copied().unwrap_or(OTROS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions() {
        let cases = [
            ("mp4", VIDEO),
            ("MKV", VIDEO),
            (".mov", VIDEO),
            ("jpg", IMAGEN),
            ("CR3", IMAGEN),
            ("flac", AUDIO),
            ("zip", COMPRIMIDO),
            ("vhdx", COMPRIMIDO),
            ("iso", COMPRIMIDO),
            ("pdf", DOCUMENTO),
            ("xlsx", DOCUMENTO),
            ("rs", CODIGO),
            ("ts", CODIGO),
            ("json", CODIGO),
            ("dll", PROGRAMA),
            ("exe", PROGRAMA),
            ("pdb", PROGRAMA),
            ("sys", SISTEMA),
            ("log", SISTEMA),
            ("regtrans-ms", SISTEMA),
            ("uasset", JUEGO),
            ("umap", JUEGO),
            ("pak", JUEGO),
            ("ucas", JUEGO),
            ("utoc", JUEGO),
            ("vpk", JUEGO),
            ("bsa", JUEGO),
            ("unity3d", JUEGO),
            ("sqlite", DATOS),
            ("pst", DATOS),
            ("safetensors", DATOS),
            ("gguf", DATOS),
            ("blend", DISENO),
            ("psd", DISENO),
            ("ttf", DISENO),
        ];
        for (e, c) in cases {
            assert_eq!(category(e), c, "{e}");
        }
    }

    #[test]
    fn unknown_and_invalid() {
        for e in ["", ".", "zzz", "dat", "bin", "una extension", "ñ", "abcdefghijklmnopq", "mp4 ", "\u{0}"] {
            assert_eq!(category(e), OTROS, "{e:?}");
        }
    }

    #[test]
    fn every_listed_extension_round_trips() {
        let mut n = 0;
        for &(cat, list) in GROUPS {
            assert!((cat as usize) < COUNT && cat != OTROS);
            assert!(!list.is_empty());
            for e in list {
                assert_eq!(category(e), cat, "{e}");
                assert_eq!(category(&e.to_ascii_uppercase()), cat, "{e}");
                n += 1;
            }
        }
        // la tabla no debe llenarse (sondeo lineal)
        assert!(n * 2 < SLOTS, "{n} extensiones");
    }

    #[test]
    fn labels_and_ids() {
        assert_eq!(CATEGORIES.len(), COUNT);
        assert_eq!(id(VIDEO), "video");
        assert_eq!(label(JUEGO), "Juegos y recursos");
        assert_eq!(label(200), "Otros");
        let mut ids: Vec<&str> = CATEGORIES.iter().map(|c| c.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COUNT);
    }

    #[test]
    fn nodes_use_flags_and_ext_table() {
        let mut b = crate::tree::Builder::default();
        b.add(1, 0, "", 0, 0, 0, flags::DIR);
        b.add(2, 1, "peli.MKV", 10, 4096, 0, 0);
        b.add(3, 1, "$MFT", 10, 4096, 0, flags::METAFILE);
        b.add(4, 1, "LEEME", 10, 4096, 0, 0);
        let t = b.build(1, "C:\\");
        let cats = ext_categories(&t);
        let find = |name: &str| t.children(t.root).iter().copied().find(|&c| t.name(c) == name).unwrap();
        assert_eq!(node_category(&t, &cats, find("peli.MKV")), VIDEO);
        assert_eq!(node_category(&t, &cats, find("$MFT")), SISTEMA);
        assert_eq!(node_category(&t, &cats, find("LEEME")), OTROS);
    }
}
