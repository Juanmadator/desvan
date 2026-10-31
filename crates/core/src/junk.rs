//! Sugerencias de limpieza: reglas sobre el árbol ya escaneado, sin tocar el disco.
//!
//! Dos tipos de reglas:
//! - Rutas conocidas ancladas a la raíz del volumen (`Users\*\AppData\Local\Temp`,
//!   `Windows\SoftwareDistribution\Download`…). Los usuarios se resuelven por la ruta
//!   dentro del árbol (`*`), no por variables de entorno, así que vale para cualquier
//!   usuario del volumen y aunque se haya escaneado solo una subcarpeta.
//! - Reglas de contexto que miran a los vecinos: `target` junto a un `Cargo.toml`,
//!   cachés de un proyecto de Unreal junto a su `.uproject`, `Library` de Unity,
//!   `steamapps\shadercache` en cualquier biblioteca de Steam, `node_modules` viejos…
//!
//! Una carpeta que coincide con una regla no se recorre por dentro: nada se cuenta dos veces.

use crate::tree::{flags, NodeId, Tree};
use serde::Serialize;
use std::collections::HashSet;

/// Qué tan seguro es borrarlo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Se regenera solo o no hace falta: se puede borrar.
    Seguro,
    /// Probablemente sobra, pero conviene mirarlo antes.
    Revisar,
    /// No se borra: se explica qué es y cómo reducirlo.
    Informativo,
}

/// Una sugerencia: todos los nodos que cumplen una regla.
#[derive(Clone, Debug, Serialize)]
pub struct JunkHit {
    pub rule: &'static str,
    pub title: String,
    pub description: String,
    pub risk: Risk,
    /// Carpetas o archivos afectados, de mayor a menor.
    pub nodes: Vec<NodeId>,
    /// Tamaño en disco total (los enlaces duros secundarios no suman).
    pub bytes: u64,
}

struct Rule {
    id: &'static str,
    title: &'static str,
    risk: Risk,
    desc: &'static str,
}

use Risk::{Informativo, Revisar, Seguro};

#[rustfmt::skip]
static RULES: &[Rule] = &[
    Rule { id: "temp_usuario", title: "Archivos temporales de usuario", risk: Seguro,
        desc: "Carpeta Temp de cada usuario (AppData\\Local\\Temp). Los programas dejan aquí restos de instalaciones y trabajos a medias. Se puede vaciar sin problema: lo que esté en uso no se dejará borrar y basta con saltarlo." },
    Rule { id: "temp_windows", title: "Archivos temporales de Windows", risk: Seguro,
        desc: "Windows\\Temp: temporales del sistema y de instaladores. Se puede vaciar (hacen falta permisos de administrador); lo que esté en uso se salta." },
    Rule { id: "cache_navegadores", title: "Caché de navegadores", risk: Seguro,
        desc: "Caché de Chrome, Edge, Brave, Opera, Vivaldi y Firefox (Cache, Code Cache, GPUCache, cache2…). Solo acelera la carga de páginas y se vuelve a crear sola. Cierra el navegador antes de borrarla; no toca contraseñas, historial ni marcadores." },
    Rule { id: "cache_aplicaciones", title: "Caché de aplicaciones (Electron)", risk: Seguro,
        desc: "Cachés de aplicaciones hechas con Electron o Chromium (Discord, Teams, Slack, VS Code…). Se regeneran solas; cierra la aplicación antes de borrarlas." },
    Rule { id: "windows_update", title: "Descargas de Windows Update", risk: Seguro,
        desc: "Windows\\SoftwareDistribution\\Download guarda actualizaciones ya descargadas. Una vez instaladas sobran. Lo más limpio es el Liberador de espacio (Limpieza de actualizaciones de Windows) o detener el servicio wuauserv antes de vaciarla." },
    Rule { id: "optimizacion_entrega", title: "Caché de Optimización de distribución", risk: Seguro,
        desc: "Trozos de actualizaciones que Windows comparte con otros equipos. Se borra desde Configuración > Sistema > Almacenamiento > Archivos temporales (Archivos de optimización de distribución)." },
    Rule { id: "volcados", title: "Volcados de memoria (.dmp)", risk: Seguro,
        desc: "Copias de la memoria guardadas tras un cuelgue o una pantalla azul (MEMORY.DMP, Minidump, LiveKernelReports, CrashDumps, *.dmp). Solo sirven para diagnosticar ese fallo: si no lo estás investigando, se pueden borrar." },
    Rule { id: "informes_errores", title: "Informes de errores de Windows", risk: Seguro,
        desc: "Informes pendientes y archivados del Informe de errores de Windows (WER). Se pueden borrar sin consecuencias." },
    Rule { id: "miniaturas", title: "Caché de miniaturas e iconos", risk: Seguro,
        desc: "Bases de datos de miniaturas e iconos del Explorador (thumbcache_*.db, iconcache_*.db, IconCache.db). Se regeneran al navegar. El Explorador tiene alguna abierta: mejor borrarlas con el Liberador de espacio (Miniaturas)." },
    Rule { id: "papelera", title: "Papelera de reciclaje", risk: Revisar,
        desc: "Archivos borrados que aún se pueden recuperar ($Recycle.Bin). Vacía la papelera si no necesitas nada de lo que hay dentro." },
    Rule { id: "windows_old", title: "Instalación anterior de Windows", risk: Revisar,
        desc: "Windows.old guarda el sistema anterior a la última actualización grande y permite volver atrás durante unos días. Si todo funciona, bórrala con el Liberador de espacio (Instalaciones anteriores de Windows); a mano suele fallar por permisos." },
    Rule { id: "restos_actualizacion", title: "Restos de actualizaciones de Windows", risk: Revisar,
        desc: "Carpetas temporales del asistente de actualización ($WINDOWS.~BT, $Windows.~WS, $GetCurrent, $SysReset, Windows10Upgrade). Si no hay una actualización en curso sobran; mejor con el Liberador de espacio (Archivos temporales de instalación de Windows)." },
    Rule { id: "descargas_instaladores", title: "Instaladores y comprimidos antiguos en Descargas", risk: Revisar,
        desc: "Instaladores (.exe, .msi, .msix…) y comprimidos (.zip, .7z, .rar, .iso) de Descargas con más de 60 días. Normalmente ya se instalaron o se extrajeron; comprueba que no sean tu única copia de algo." },
    Rule { id: "npm_cache", title: "Caché de npm", risk: Seguro,
        desc: "Paquetes descargados por npm (npm-cache). Se vuelven a descargar cuando hacen falta. Comando oficial: «npm cache clean --force»." },
    Rule { id: "yarn_cache", title: "Caché de Yarn", risk: Seguro,
        desc: "Paquetes descargados por Yarn. Se regeneran solos. Comando: «yarn cache clean»." },
    Rule { id: "pnpm_store", title: "Almacén de pnpm", risk: Revisar,
        desc: "Almacén global de paquetes de pnpm. Los proyectos enlazan a él: en vez de borrarlo, «pnpm store prune» quita lo que ya no usa ningún proyecto." },
    Rule { id: "node_modules", title: "node_modules sin usar (más de 90 días)", risk: Revisar,
        desc: "Dependencias de proyectos JavaScript que no se tocan desde hace más de 90 días. Se recuperan con «npm install» (o yarn/pnpm) cuando vuelvas al proyecto." },
    Rule { id: "rust_target", title: "Compilaciones de Rust (target)", risk: Revisar,
        desc: "Carpetas target de proyectos Rust (junto a un Cargo.toml). Solo guardan resultados de compilación: «cargo clean» las borra y la siguiente compilación las rehace (tardará más)." },
    Rule { id: "cargo_registro", title: "Registro de Cargo", risk: Revisar,
        desc: "Paquetes de Rust descargados y descomprimidos por Cargo (.cargo\\registry). Se vuelven a descargar al compilar; sin conexión no se podrá." },
    Rule { id: "gradle_cache", title: "Caché de Gradle", risk: Seguro,
        desc: "Dependencias y compilaciones guardadas por Gradle (.gradle\\caches). Se vuelven a descargar en la siguiente compilación." },
    Rule { id: "pip_cache", title: "Caché de pip", risk: Seguro,
        desc: "Paquetes de Python descargados por pip. Comando oficial: «pip cache purge»." },
    Rule { id: "nuget_paquetes", title: "Paquetes de NuGet", risk: Revisar,
        desc: "Carpeta global de paquetes de NuGet (.nuget\\packages). Se restauran al compilar, pero sin conexión los proyectos no compilarán. Comando: «dotnet nuget locals all --clear»." },
    Rule { id: "nuget_cache", title: "Caché de NuGet", risk: Seguro,
        desc: "Caché HTTP y de complementos de NuGet. Se regenera sola." },
    Rule { id: "go_cache", title: "Caché de compilación de Go", risk: Seguro,
        desc: "go-build: resultados de compilación de Go. Comando: «go clean -cache»." },
    Rule { id: "go_modulos", title: "Módulos de Go", risk: Revisar,
        desc: "Código descargado de las dependencias de Go (go\\pkg\\mod). Se vuelve a descargar al compilar. Comando: «go clean -modcache»." },
    Rule { id: "unreal_cache", title: "Cachés de proyectos de Unreal", risk: Seguro,
        desc: "DerivedDataCache e Intermediate de proyectos de Unreal Engine (junto a un .uproject) y de sus plugins. Unreal los regenera al abrir o compilar; la primera apertura tardará más (shaders)." },
    Rule { id: "unreal_logs", title: "Registros y cuelgues de Unreal", risk: Seguro,
        desc: "Saved\\Logs y Saved\\Crashes de proyectos de Unreal: registros e informes de cuelgues de sesiones pasadas." },
    Rule { id: "unreal_binarios", title: "Binarios de proyectos de Unreal", risk: Revisar,
        desc: "Carpeta Binaries de proyectos de Unreal: se rehace al compilar. Bórrala solo si el proyecto tiene su código (carpeta Source) y puedes compilarlo." },
    Rule { id: "unreal_ddc_global", title: "Caché global de Unreal Engine", risk: Seguro,
        desc: "DerivedDataCache compartida de Unreal Engine (AppData\\Local\\UnrealEngine\\Common, incluido el almacén de Zen). Se regenera; los proyectos tardarán más en abrir la primera vez." },
    Rule { id: "epic_vaultcache", title: "Caché de Fab / Marketplace (VaultCache)", risk: Revisar,
        desc: "Copias de los recursos de Fab/Marketplace descargados con el Epic Games Launcher. Si vuelves a añadirlos a un proyecto, se descargarán de nuevo." },
    Rule { id: "godot_import", title: "Importaciones de Godot", risk: Seguro,
        desc: "Recursos importados de proyectos de Godot (.godot\\imported o .import, junto a project.godot). Godot los vuelve a importar al abrir el proyecto." },
    Rule { id: "unity_library", title: "Library de proyectos de Unity", risk: Seguro,
        desc: "Carpeta Library de proyectos de Unity (junto a Assets y ProjectSettings): es una caché de importación. Unity la regenera al abrir el proyecto, lo que puede tardar en proyectos grandes." },
    Rule { id: "unity_cache", title: "Caché global de Unity", risk: Seguro,
        desc: "Caché de paquetes y del Asset Store de Unity (AppData\\Local\\Unity\\cache). Se vuelve a descargar cuando hace falta." },
    Rule { id: "steam_shaders", title: "Caché de shaders de Steam", risk: Seguro,
        desc: "steamapps\\shadercache: shaders precompilados de los juegos. Steam los vuelve a generar o descargar; los primeros minutos de juego pueden ir a tirones." },
    Rule { id: "steam_htmlcache", title: "Caché web de Steam", risk: Seguro,
        desc: "Caché del navegador integrado de Steam (htmlcache). Se regenera; cierra Steam antes de borrarla." },
    Rule { id: "epic_webcache", title: "Caché web de Epic Games Launcher", risk: Seguro,
        desc: "Caché del navegador integrado del Epic Games Launcher (webcache). Se regenera; cierra el launcher antes." },
    Rule { id: "cache_shaders_gpu", title: "Caché de shaders de la tarjeta gráfica", risk: Seguro,
        desc: "Shaders compilados por los controladores de NVIDIA (DXCache, GLCache), AMD, Intel y DirectX (D3DSCache). Se regeneran solos; algún juego tardará más en cargar la primera vez." },
    Rule { id: "instaladores_graficos", title: "Instaladores descomprimidos de controladores", risk: Revisar,
        desc: "Carpetas NVIDIA y AMD en la raíz de la unidad que dejan los instaladores de controladores gráficos. Los controladores ya instalados no las necesitan." },
    Rule { id: "registros_windows", title: "Registros de Windows", risk: Revisar,
        desc: "Registros del sistema (Windows\\Logs, System32\\LogFiles, Panther). A veces crecen mucho (CBS); sirven para diagnosticar problemas, pero Windows no los necesita para funcionar." },
    Rule { id: "parches_instalador", title: "Caché de parches de Windows Installer", risk: Revisar,
        desc: "Windows\\Installer\\$PatchCache$: copias de archivos para aplicar parches a programas MSI. Se puede borrar, pero algún parche o reparación pedirá el instalador original. El resto de Windows\\Installer no se toca." },
    Rule { id: "package_cache", title: "Caché de instaladores de programas", risk: Revisar,
        desc: "ProgramData\\Package Cache guarda los instaladores de Visual Studio, .NET, redistribuibles de C++… Sin ellos, desinstalar o reparar esos programas puede fallar. Mejor no tocarla salvo que sepas qué hay." },
    Rule { id: "chkdsk", title: "Fragmentos recuperados por chkdsk", risk: Revisar,
        desc: "Carpetas FOUND.000, FOUND.001… que crea chkdsk con trozos de archivos dañados (*.chk). Casi nunca sirven: échales un vistazo y bórralas." },
    Rule { id: "discos_virtuales", title: "Discos virtuales de WSL y Docker", risk: Informativo,
        desc: "Discos .vhdx de WSL o Docker Desktop. Crecen pero no se encogen solos al borrar datos dentro. Para recuperar espacio: «wsl --shutdown» y compactarlos con Optimize-VHD o diskpart (compact vdisk). No los borres: perderías esas distribuciones." },
    Rule { id: "winsxs", title: "Almacén de componentes (WinSxS)", risk: Informativo,
        desc: "Windows\\WinSxS guarda los componentes del sistema; muchos son enlaces duros que no ocupan doble. Nunca se borra a mano: «Dism /Online /Cleanup-Image /StartComponentCleanup» quita versiones antiguas." },
    Rule { id: "restauracion", title: "Puntos de restauración e instantáneas", risk: Informativo,
        desc: "System Volume Information guarda puntos de restauración e instantáneas de volumen. Se gestiona desde Protección del sistema (Configurar > Uso máximo) o con «vssadmin resize shadowstorage»." },
    Rule { id: "hiberfil", title: "Archivo de hibernación (hiberfil.sys)", risk: Informativo,
        desc: "Guarda la memoria al hibernar y para el inicio rápido. No se borra a mano: «powercfg /h /type reduced» lo reduce (solo inicio rápido) y «powercfg /h off» lo elimina (sin hibernación ni inicio rápido)." },
    Rule { id: "pagefile", title: "Archivo de paginación (pagefile.sys)", risk: Informativo,
        desc: "Memoria virtual de Windows. No se borra: su tamaño se ajusta en Configuración avanzada del sistema > Rendimiento > Opciones avanzadas > Memoria virtual. Quitarlo puede cerrar programas por falta de memoria." },
    Rule { id: "swapfile", title: "Archivo de intercambio (swapfile.sys)", risk: Informativo,
        desc: "Lo usa Windows para suspender aplicaciones de la Tienda. Suele ser pequeño y Windows lo gestiona junto con el archivo de paginación; no se borra." },
];

/// Carpetas conocidas, relativas a la raíz del volumen. `*` = un componente cualquiera;
/// `abc*` = componente que empieza por `abc`. Sin distinguir mayúsculas.
#[rustfmt::skip]
static PATTERNS: &[(&str, &str)] = &[
    ("temp_usuario", r"Users\*\AppData\Local\Temp"),
    ("temp_windows", r"Windows\Temp"),
    ("windows_update", r"Windows\SoftwareDistribution\Download"),
    ("optimizacion_entrega", r"Windows\SoftwareDistribution\DeliveryOptimization"),
    ("optimizacion_entrega", r"Windows\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"),
    ("optimizacion_entrega", r"ProgramData\Microsoft\Windows\DeliveryOptimization\Cache"),
    ("volcados", r"Windows\Minidump"),
    ("volcados", r"Windows\LiveKernelReports"),
    ("volcados", r"Users\*\AppData\Local\CrashDumps"),
    ("informes_errores", r"ProgramData\Microsoft\Windows\WER\ReportArchive"),
    ("informes_errores", r"ProgramData\Microsoft\Windows\WER\ReportQueue"),
    ("informes_errores", r"ProgramData\Microsoft\Windows\WER\Temp"),
    ("informes_errores", r"Users\*\AppData\Local\Microsoft\Windows\WER\ReportArchive"),
    ("informes_errores", r"Users\*\AppData\Local\Microsoft\Windows\WER\ReportQueue"),
    ("papelera", r"$Recycle.Bin"),
    ("windows_old", r"Windows.old"),
    ("restos_actualizacion", r"$WINDOWS.~BT"),
    ("restos_actualizacion", r"$Windows.~WS"),
    ("restos_actualizacion", r"$GetCurrent"),
    ("restos_actualizacion", r"$SysReset"),
    ("restos_actualizacion", r"Windows10Upgrade"),
    ("instaladores_graficos", r"NVIDIA"),
    ("instaladores_graficos", r"AMD"),
    ("restauracion", r"System Volume Information"),
    ("winsxs", r"Windows\WinSxS"),
    ("registros_windows", r"Windows\Logs"),
    ("registros_windows", r"Windows\Panther"),
    ("registros_windows", r"Windows\System32\LogFiles"),
    ("parches_instalador", r"Windows\Installer\$PatchCache$"),
    ("package_cache", r"ProgramData\Package Cache"),
    ("epic_vaultcache", r"ProgramData\Epic\EpicGamesLauncher\VaultCache"),
    // navegadores (Chromium: perfiles en User Data\<perfil>)
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\*\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\*\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\*\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\*\DawnCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\*\DawnGraphiteCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\*\DawnWebGPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\ShaderCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Google\Chrome*\User Data\GrShaderCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\*\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\*\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\*\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\*\DawnCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\*\DawnGraphiteCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\*\DawnWebGPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\ShaderCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Edge*\User Data\GrShaderCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\BraveSoftware\Brave-Browser*\User Data\*\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\BraveSoftware\Brave-Browser*\User Data\*\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\BraveSoftware\Brave-Browser*\User Data\*\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\BraveSoftware\Brave-Browser*\User Data\ShaderCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Vivaldi\User Data\*\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Vivaldi\User Data\*\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Vivaldi\User Data\*\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Chromium\User Data\*\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Chromium\User Data\*\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Chromium\User Data\*\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Opera Software\*\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Opera Software\*\Default\Cache"),
    ("cache_navegadores", r"Users\*\AppData\Roaming\Opera Software\*\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Roaming\Opera Software\*\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Roaming\Opera Software\*\Default\Code Cache"),
    ("cache_navegadores", r"Users\*\AppData\Roaming\Opera Software\*\Default\GPUCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Mozilla\Firefox\Profiles\*\cache2"),
    ("cache_navegadores", r"Users\*\AppData\Local\Mozilla\Firefox\Profiles\*\startupCache"),
    ("cache_navegadores", r"Users\*\AppData\Local\Microsoft\Windows\INetCache"),
    // desarrollo
    ("npm_cache", r"Users\*\AppData\Local\npm-cache"),
    ("npm_cache", r"Users\*\AppData\Roaming\npm-cache"),
    ("yarn_cache", r"Users\*\AppData\Local\Yarn\Cache"),
    ("yarn_cache", r"Users\*\AppData\Local\Yarn\Berry\cache"),
    ("pnpm_store", r"Users\*\AppData\Local\pnpm\store"),
    ("pnpm_store", r"Users\*\AppData\Local\pnpm-store"),
    ("pnpm_store", r".pnpm-store"),
    ("cargo_registro", r"Users\*\.cargo\registry\cache"),
    ("cargo_registro", r"Users\*\.cargo\registry\src"),
    ("gradle_cache", r"Users\*\.gradle\caches"),
    ("pip_cache", r"Users\*\AppData\Local\pip\cache"),
    ("nuget_paquetes", r"Users\*\.nuget\packages"),
    ("nuget_cache", r"Users\*\AppData\Local\NuGet\v3-cache"),
    ("nuget_cache", r"Users\*\AppData\Local\NuGet\Cache"),
    ("nuget_cache", r"Users\*\AppData\Local\NuGet\plugins-cache"),
    ("go_cache", r"Users\*\AppData\Local\go-build"),
    ("go_modulos", r"Users\*\go\pkg\mod"),
    // motores y lanzadores de juegos
    ("unreal_ddc_global", r"Users\*\AppData\Local\UnrealEngine\Common\DerivedDataCache"),
    ("unreal_ddc_global", r"Users\*\AppData\Local\UnrealEngine\Common\Zen\Data"),
    ("unity_cache", r"Users\*\AppData\Local\Unity\cache"),
    ("steam_htmlcache", r"Users\*\AppData\Local\Steam\htmlcache"),
    ("epic_webcache", r"Users\*\AppData\Local\EpicGamesLauncher\Saved\webcache*"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\NVIDIA\DXCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\NVIDIA\GLCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\LocalLow\NVIDIA\PerDriverVersion\DXCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\LocalLow\NVIDIA\PerDriverVersion\GLCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\NVIDIA Corporation\NV_Cache"),
    ("cache_shaders_gpu", r"ProgramData\NVIDIA Corporation\NV_Cache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\D3DSCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\AMD\DxCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\AMD\DxcCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\AMD\GLCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\AMD\VkCache"),
    ("cache_shaders_gpu", r"Users\*\AppData\Local\Intel\ShaderCache"),
];

// Marcas de contexto en el autómata.
/// `Users\<u>\Downloads` (vale para todo lo que cuelga).
const M_DOWNLOADS: u8 = 1;
/// `Users\<u>\AppData\Local` (vale para todo lo que cuelga).
const M_LOCAL: u8 = 2;
/// Carpeta de cachés del Explorador.
const M_EXPLORER: u8 = 4;
/// Carpeta de una aplicación en `AppData\Roaming` (posible Electron).
const M_ROAMING_APP: u8 = 8;
/// Marcas que heredan los descendientes.
const INHERITED: u8 = M_DOWNLOADS | M_LOCAL;

static MARKS: &[(u8, &str)] = &[
    (M_DOWNLOADS, r"Users\*\Downloads"),
    (M_LOCAL, r"Users\*\AppData\Local"),
    (M_EXPLORER, r"Users\*\AppData\Local\Microsoft\Windows\Explorer"),
    (M_ROAMING_APP, r"Users\*\AppData\Roaming\*"),
];

/// Cachés con nombre de Chromium que delatan una aplicación Electron.
const ELECTRON_CACHES: &[&str] = &["Cache", "Code Cache", "GPUCache", "DawnCache", "DawnGraphiteCache", "DawnWebGPUCache"];
const INSTALLER_EXTS: &[&str] = &["exe", "msi", "msix", "msixbundle", "appx", "appxbundle", "zip", "7z", "rar", "iso"];
const DAY: i64 = 86_400;

#[derive(Clone, Copy)]
enum Seg {
    Lit(&'static str),
    Prefix(&'static str),
    Any,
}

impl Seg {
    fn parse(s: &'static str) -> Seg {
        match s {
            "*" => Seg::Any,
            _ if s.ends_with('*') => Seg::Prefix(&s[..s.len() - 1]),
            _ => Seg::Lit(s),
        }
    }
    fn matches(self, name: &str) -> bool {
        match self {
            Seg::Any => true,
            Seg::Lit(l) => name.eq_ignore_ascii_case(l),
            Seg::Prefix(p) => name.len() >= p.len() && name.is_char_boundary(p.len()) && name[..p.len()].eq_ignore_ascii_case(p),
        }
    }
    fn same(self, other: Seg) -> bool {
        match (self, other) {
            (Seg::Any, Seg::Any) => true,
            (Seg::Lit(a), Seg::Lit(b)) | (Seg::Prefix(a), Seg::Prefix(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

#[derive(Default)]
struct TNode {
    kids: Vec<(Seg, u32)>,
    accept: Option<u8>,
    mark: u8,
}

/// Autómata de rutas: cada estado es un nodo del trie de patrones.
struct Trie {
    nodes: Vec<TNode>,
}

impl Trie {
    fn build() -> Trie {
        let mut t = Trie { nodes: vec![TNode::default()] };
        for &(id, pat) in PATTERNS {
            let r = RULES.iter().position(|r| r.id == id).unwrap_or(0) as u8;
            let n = t.insert(pat);
            t.nodes[n].accept.get_or_insert(r);
        }
        for &(m, pat) in MARKS {
            let n = t.insert(pat);
            t.nodes[n].mark |= m;
        }
        t
    }

    fn insert(&mut self, pat: &'static str) -> usize {
        let mut cur = 0usize;
        for part in pat.split('\\') {
            let seg = Seg::parse(part);
            cur = match self.nodes[cur].kids.iter().find(|(s, _)| s.same(seg)) {
                Some(&(_, k)) => k as usize,
                None => {
                    self.nodes.push(TNode::default());
                    let k = self.nodes.len() - 1;
                    self.nodes[cur].kids.push((seg, k as u32));
                    k
                }
            };
        }
        cur
    }

    fn step(&self, states: &[u32], name: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for &s in states {
            for &(seg, k) in &self.nodes[s as usize].kids {
                if seg.matches(name) && !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }

    fn accept(&self, states: &[u32]) -> Option<usize> {
        states.iter().filter_map(|&s| self.nodes[s as usize].accept).min().map(|r| r as usize)
    }

    fn marks(&self, states: &[u32]) -> u8 {
        states.iter().fold(0, |m, &s| m | self.nodes[s as usize].mark)
    }
}

/// Componentes de la ruta de la raíz relativos a la raíz del volumen
/// (`C:\Users\ana` → `["Users", "ana"]`, `\\srv\rec\x` → `["x"]`).
fn volume_components(root_path: &str) -> Vec<String> {
    let s = root_path.replace('/', "\\");
    let s = s.strip_prefix(r"\\?\").map(|r| r.strip_prefix(r"UNC\").map(|u| format!(r"\\{u}")).unwrap_or_else(|| r.to_string())).unwrap_or(s);
    let rest: Vec<&str> = if let Some(unc) = s.strip_prefix(r"\\") {
        unc.split('\\').filter(|p| !p.is_empty()).skip(2).collect()
    } else if s.as_bytes().get(1) == Some(&b':') {
        s[2..].split('\\').filter(|p| !p.is_empty()).collect()
    } else {
        s.split('\\').filter(|p| !p.is_empty()).collect()
    };
    rest.into_iter().map(str::to_string).collect()
}

/// Sugerencias de limpieza para `tree` (`root_path` es la ruta de su raíz, p. ej. `C:\`).
pub fn find_junk(tree: &Tree, root_path: &str) -> Vec<JunkHit> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    find_junk_at(tree, root_path, now)
}

/// Como [`find_junk`], con la fecha actual dada (segundos Unix) para las reglas de antigüedad.
pub fn find_junk_at(tree: &Tree, root_path: &str, now: i64) -> Vec<JunkHit> {
    if tree.is_empty() {
        return Vec::new();
    }
    let trie = Trie::build();
    let prefix = volume_components(root_path);
    let mut states = vec![0u32];
    let mut inherited = 0u8;
    for c in &prefix {
        inherited |= trie.marks(&states) & INHERITED;
        states = trie.step(&states, c);
    }
    let mut s = Scanner { tree, now, trie: &trie, hits: vec![Vec::new(); RULES.len()], claimed: HashSet::new() };
    if let Some(r) = trie.accept(&states) {
        // la propia raíz escaneada ya es una carpeta conocida
        s.hit(r, tree.root);
    } else {
        s.walk(states, inherited, prefix.is_empty());
    }
    s.finish()
}

struct Scanner<'a> {
    tree: &'a Tree,
    now: i64,
    trie: &'a Trie,
    hits: Vec<Vec<NodeId>>,
    /// Nietos ya asignados a una regla desde su abuelo (Saved\Logs, .godot\imported…).
    claimed: HashSet<NodeId>,
}

fn rule(id: &str) -> usize {
    RULES.iter().position(|r| r.id == id).unwrap_or(0)
}

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl Scanner<'_> {
    fn hit(&mut self, r: usize, n: NodeId) {
        self.hits[r].push(n);
    }

    fn claim(&mut self, id: &str, n: NodeId) {
        self.hits[rule(id)].push(n);
        self.claimed.insert(n);
    }

    fn is_dir(&self, n: NodeId) -> bool {
        self.tree.is_dir(n)
    }

    /// Hijo (carpeta) de `n` con ese nombre.
    fn child_dir(&self, n: NodeId, name: &str) -> Option<NodeId> {
        self.tree.children(n).iter().copied().find(|&c| self.is_dir(c) && eq(self.tree.name(c), name))
    }

    /// Fecha más reciente de todo lo que cuelga de `n` (incluido).
    fn newest(&self, n: NodeId) -> i64 {
        let t = self.tree;
        let mut best = t.mtime[n as usize];
        let mut stack = vec![n];
        while let Some(d) = stack.pop() {
            for &c in t.children(d) {
                best = best.max(t.mtime[c as usize]);
                if t.is_dir(c) {
                    stack.push(c);
                }
            }
        }
        best
    }

    /// Recorrido en profundidad con pila propia (sin recursión: los árboles pueden ser muy hondos).
    fn walk(&mut self, root_states: Vec<u32>, root_inherited: u8, root_is_volume: bool) {
        let t = self.tree;
        let mut stack: Vec<(NodeId, Vec<u32>, u8)> = vec![(t.root, root_states, root_inherited)];
        while let Some((dir, states, inherited)) = stack.pop() {
            let here = self.trie.marks(&states);
            let inh = inherited | (here & INHERITED);
            let vol_root = root_is_volume && dir == t.root;
            let kids = t.children(dir);

            // marcadores de proyecto entre los hijos
            let (mut cargo, mut uproject, mut godot, mut steam, mut assets, mut settings, mut electron) = (false, false, false, false, false, false, 0);
            for &c in kids {
                let n = t.name(c);
                if t.is_dir(c) {
                    assets |= eq(n, "Assets");
                    settings |= eq(n, "ProjectSettings");
                    if here & M_ROAMING_APP != 0 && ELECTRON_CACHES.iter().any(|e| eq(n, e)) {
                        electron += 1;
                    }
                } else {
                    cargo |= eq(n, "Cargo.toml");
                    uproject |= t.ext_name(c) == "uproject";
                    godot |= eq(n, "project.godot");
                    steam |= eq(n, "steam.exe");
                }
            }
            let parent_name = t.name(dir);

            for &c in kids {
                let f = t.flags[c as usize];
                if f & (flags::SYNTHETIC | flags::METAFILE) != 0 || c == t.orphans {
                    continue;
                }
                let name = t.name(c);
                if !t.is_dir(c) {
                    self.file(c, name, here, inh, vol_root);
                    continue;
                }
                if self.claimed.contains(&c) {
                    continue; // ya asignada desde su abuelo
                }
                let st = self.trie.step(&states, name);
                if let Some(r) = self.trie.accept(&st) {
                    self.hit(r, c);
                    continue;
                }
                // reglas de contexto
                let ctx = if cargo && eq(name, "target") && self.looks_like_cargo_target(c) {
                    Some("rust_target")
                } else if uproject && (eq(name, "DerivedDataCache") || eq(name, "Intermediate")) {
                    Some("unreal_cache")
                } else if uproject && eq(name, "Binaries") {
                    Some("unreal_binarios")
                } else if godot && eq(name, ".import") {
                    Some("godot_import")
                } else if assets && settings && eq(name, "Library") {
                    Some("unity_library")
                } else if eq(parent_name, "steamapps") && eq(name, "shadercache") {
                    Some("steam_shaders")
                } else if electron >= 2 && ELECTRON_CACHES.iter().any(|e| eq(name, e)) {
                    Some("cache_aplicaciones")
                } else if vol_root && is_found_dir(name) {
                    Some("chkdsk")
                } else {
                    None
                };
                if let Some(id) = ctx {
                    self.hit(rule(id), c);
                    continue;
                }
                if eq(name, "node_modules") {
                    // no se baja nunca: miles de paquetes sin nada más que buscar
                    if self.newest(c) < self.now - 90 * DAY {
                        self.hit(rule("node_modules"), c);
                    }
                    continue;
                }
                // nietos con regla (se reservan para no recorrerlos después)
                if uproject && eq(name, "Saved") {
                    for sub in ["Logs", "Crashes"] {
                        if let Some(g) = self.child_dir(c, sub) {
                            self.claim("unreal_logs", g);
                        }
                    }
                } else if uproject && eq(name, "Plugins") {
                    for &plugin in t.children(c) {
                        if let Some(g) = self.child_dir(plugin, "Intermediate") {
                            self.claim("unreal_cache", g);
                        }
                    }
                } else if godot && eq(name, ".godot") {
                    if let Some(g) = self.child_dir(c, "imported") {
                        self.claim("godot_import", g);
                    }
                } else if steam && eq(name, "config") {
                    if let Some(g) = self.child_dir(c, "htmlcache") {
                        self.claim("steam_htmlcache", g);
                    }
                }
                if f & (flags::REPARSE | flags::DENIED) == 0 {
                    stack.push((c, st, inh));
                }
            }
        }
    }

    /// Reglas de archivos sueltos.
    fn file(&mut self, c: NodeId, name: &str, here: u8, inh: u8, vol_root: bool) {
        let t = self.tree;
        if t.flags[c as usize] & flags::HARDLINK != 0 {
            return;
        }
        let ext = t.ext_name(c);
        let id = if ext == "dmp" {
            "volcados"
        } else if vol_root && eq(name, "hiberfil.sys") {
            "hiberfil"
        } else if vol_root && eq(name, "pagefile.sys") {
            "pagefile"
        } else if vol_root && eq(name, "swapfile.sys") {
            "swapfile"
        } else if (here & M_EXPLORER != 0 && ext == "db" && (starts_ci(name, "thumbcache_") || starts_ci(name, "iconcache_"))) || (here & M_LOCAL != 0 && eq(name, "IconCache.db")) {
            "miniaturas"
        } else if inh & M_DOWNLOADS != 0 && INSTALLER_EXTS.contains(&ext) && t.mtime[c as usize] < self.now - 60 * DAY {
            "descargas_instaladores"
        } else if inh & M_LOCAL != 0 && ext == "vhdx" {
            "discos_virtuales"
        } else {
            return;
        };
        self.hit(rule(id), c);
    }

    /// Una carpeta `target` de Cargo tiene CACHEDIR.TAG, .rustc_info.json, debug o release.
    fn looks_like_cargo_target(&self, n: NodeId) -> bool {
        self.tree.children(n).iter().any(|&c| {
            let m = self.tree.name(c);
            eq(m, "CACHEDIR.TAG") || eq(m, ".rustc_info.json") || eq(m, "debug") || eq(m, "release")
        })
    }

    fn finish(self) -> Vec<JunkHit> {
        let t = self.tree;
        let mut out: Vec<JunkHit> = Vec::new();
        for (i, mut nodes) in self.hits.into_iter().enumerate() {
            nodes.sort_unstable();
            nodes.dedup();
            // lo que no ocupa nada (una Temp vacía, un enlace duro secundario) no se sugiere
            let size = |n: NodeId| if t.flags[n as usize] & flags::HARDLINK != 0 { 0 } else { t.alloc[n as usize] };
            nodes.retain(|&n| size(n) > 0);
            let bytes: u64 = nodes.iter().map(|&n| size(n)).sum();
            if nodes.is_empty() || bytes == 0 {
                continue;
            }
            nodes.sort_by(|&a, &b| size(b).cmp(&size(a)).then(a.cmp(&b)));
            let r = &RULES[i];
            out.push(JunkHit { rule: r.id, title: r.title.to_string(), description: r.desc.to_string(), risk: r.risk, nodes, bytes });
        }
        out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.rule.cmp(b.rule)));
        out
    }
}

fn starts_ci(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p)
}

/// `FOUND.000`, `FOUND.001`…
fn is_found_dir(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 9 && starts_ci(name, "FOUND.") && b[6..].iter().all(u8::is_ascii_digit)
}

/// Catálogo de reglas (identificador, título, riesgo) para mostrar en la interfaz.
pub fn rules() -> impl Iterator<Item = (&'static str, &'static str, Risk)> {
    RULES.iter().map(|r| (r.id, r.title, r.risk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_point_to_rules() {
        for (id, _) in PATTERNS {
            assert!(RULES.iter().any(|r| r.id == *id), "regla desconocida: {id}");
        }
        let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), RULES.len(), "ids repetidos");
        assert!(RULES.len() < u8::MAX as usize);
    }

    #[test]
    fn components() {
        assert!(volume_components(r"C:\").is_empty());
        assert_eq!(volume_components(r"C:\Users\ana\"), vec!["Users", "ana"]);
        assert_eq!(volume_components(r"\\srv\rec\x\y"), vec!["x", "y"]);
        assert_eq!(volume_components(r"\\?\C:\Windows"), vec!["Windows"]);
        assert_eq!(volume_components(r"\\?\UNC\srv\rec\z"), vec!["z"]);
    }

    #[test]
    fn segments() {
        assert!(Seg::parse("webcache*").matches("webcache_4147"));
        assert!(Seg::parse("webcache*").matches("WebCache"));
        assert!(!Seg::parse("webcache*").matches("web"));
        assert!(Seg::parse("Temp").matches("TEMP"));
        assert!(Seg::parse("*").matches("ñandú"));
        assert!(is_found_dir("FOUND.000") && is_found_dir("found.123") && !is_found_dir("FOUND.ABC") && !is_found_dir("FOUND.0000"));
    }
}
