//! Integración con Windows: unidades, permisos de administrador y Papelera de reciclaje.

use serde::Serialize;
use std::os::windows::ffi::OsStrExt;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, MAX_PATH};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SHFileOperationW, ShellExecuteW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct VolumeEntry {
    /// "C"
    pub letter: String,
    pub label: String,
    pub filesystem: String,
    pub total: u64,
    pub free: u64,
    /// fija, extraíble, red, óptica…
    pub kind: &'static str,
    pub cluster: u32,
}

/// Unidades con letra, montadas y legibles.
pub fn volumes() -> Vec<VolumeEntry> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wroot = wide(&root);
        let kind = match unsafe { GetDriveTypeW(PCWSTR(wroot.as_ptr())) } {
            2 => "extraíble",
            3 => "fija",
            4 => "red",
            5 => "óptica",
            6 => "ram",
            _ => continue,
        };
        let mut label = [0u16; MAX_PATH as usize + 1];
        let mut fs = [0u16; MAX_PATH as usize + 1];
        let ok = unsafe { GetVolumeInformationW(PCWSTR(wroot.as_ptr()), Some(&mut label), None, None, None, Some(&mut fs)) };
        if ok.is_err() {
            continue; // lector vacío, unidad desconectada…
        }
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        let _ = unsafe { GetDiskFreeSpaceExW(PCWSTR(wroot.as_ptr()), Some(&mut avail), Some(&mut total), Some(&mut free)) };
        let (mut spc, mut bps, mut fc, mut tc) = (0u32, 0u32, 0u32, 0u32);
        let _ = unsafe { GetDiskFreeSpaceW(PCWSTR(wroot.as_ptr()), Some(&mut spc), Some(&mut bps), Some(&mut fc), Some(&mut tc)) };
        let z = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        out.push(VolumeEntry { letter: letter.to_string(), label: z(&label), filesystem: z(&fs), total, free, kind, cluster: spc * bps });
    }
    out
}

/// ¿El proceso tiene el token elevado (administrador)?
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut elev as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len).is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

/// Vuelve a lanzar este ejecutable pidiendo permisos de administrador (aparece el aviso de UAC).
/// Devuelve `false` si el usuario lo rechaza.
pub fn relaunch_elevated(args: &str) -> bool {
    let exe = std::env::current_exe().unwrap_or_default();
    let wexe = wide(&exe.to_string_lossy());
    let wargs = wide(args);
    let verb = wide("runas");
    let r = unsafe { ShellExecuteW(Some(HWND::default()), PCWSTR(verb.as_ptr()), PCWSTR(wexe.as_ptr()), PCWSTR(wargs.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL) };
    r.0 as isize > 32
}

/// Envía archivos o carpetas a la Papelera de reciclaje (se pueden recuperar desde ella).
pub fn recycle(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    // lista terminada en doble nulo
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(std::ffi::OsStr::new(p).encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        pTo: PCWSTR::null(),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0) as u16,
        fAnyOperationsAborted: false.into(),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        return Err(format!("no se pudo mover a la Papelera (código {r:#x})"));
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("operación cancelada".into());
    }
    Ok(())
}

#[allow(dead_code)]
fn _unused(_: PWSTR) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_system_drive() {
        let v = volumes();
        assert!(v.iter().any(|d| d.letter == "C" && d.total > 0), "{v:?}");
    }

    #[test]
    fn recycles_a_temp_file() {
        let dir = std::env::temp_dir().join(format!("desvan-recycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("borrame.txt");
        std::fs::write(&f, b"hola").unwrap();
        recycle(&[f.to_string_lossy().into_owned()]).unwrap();
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---------- restaurar desde la Papelera ----------

/// Entrada de la Papelera: el archivo `$I…` describe al `$R…` que guarda el contenido.
#[derive(Clone, Debug)]
pub struct RecycledItem {
    pub original: String,
    pub deleted_unix: i64,
    pub size: u64,
    pub info_file: std::path::PathBuf,
    pub data_file: std::path::PathBuf,
}

/// Lee un archivo `$I` (versión 1 de Vista/7 y versión 2 de Windows 10+).
pub fn parse_recycle_info(b: &[u8]) -> Option<(String, i64, u64)> {
    if b.len() < 24 {
        return None;
    }
    let version = u64::from_le_bytes(b[0..8].try_into().ok()?);
    let size = u64::from_le_bytes(b[8..16].try_into().ok()?);
    let ft = u64::from_le_bytes(b[16..24].try_into().ok()?);
    let units: Vec<u16> = match version {
        1 => b.get(24..24 + 520)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
        2 => {
            let n = u32::from_le_bytes(b.get(24..28)?.try_into().ok()?) as usize;
            b.get(28..28 + n * 2)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        }
        _ => return None,
    };
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    Some((String::from_utf16_lossy(&units[..end]), crate::util::filetime_to_unix(ft), size))
}

/// Carpeta de la Papelera del usuario actual en una unidad (la única legible de `$Recycle.Bin`).
fn user_recycle_dir(letter: char) -> Option<std::path::PathBuf> {
    let base = std::path::PathBuf::from(format!("{}:\\$Recycle.Bin", letter.to_ascii_uppercase()));
    std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("S-1-5-21-"))
        .find(|e| std::fs::read_dir(e.path()).is_ok())
        .map(|e| e.path())
}

/// Lo que hay en la Papelera de una unidad.
pub fn recycled_items(letter: char) -> Vec<RecycledItem> {
    let Some(dir) = user_recycle_dir(letter) else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let rest = name.strip_prefix("$I")?;
            let bytes = std::fs::read(e.path()).ok()?;
            let (original, deleted_unix, size) = parse_recycle_info(&bytes)?;
            Some(RecycledItem { original, deleted_unix, size, info_file: e.path(), data_file: dir.join(format!("$R{rest}")) })
        })
        .collect()
}

/// Devuelve a su sitio lo enviado a la Papelera a partir de `since_unix` (el borrado más reciente
/// de cada ruta). Devuelve, por cada ruta, si se pudo restaurar.
pub fn restore(paths: &[String], since_unix: i64) -> Vec<Result<(), String>> {
    let mut by_drive: std::collections::HashMap<char, Vec<RecycledItem>> = Default::default();
    paths
        .iter()
        .map(|p| {
            let letter = p.chars().next().ok_or("ruta vacía")?.to_ascii_uppercase();
            let items = by_drive.entry(letter).or_insert_with(|| recycled_items(letter));
            let item = items
                .iter()
                .filter(|it| it.original.eq_ignore_ascii_case(p) && it.deleted_unix >= since_unix - 120)
                .max_by_key(|it| it.deleted_unix)
                .cloned()
                .ok_or_else(|| format!("«{p}» ya no está en la Papelera"))?;
            if std::path::Path::new(p).exists() {
                return Err(format!("ya existe algo en «{p}»"));
            }
            std::fs::rename(&item.data_file, p).map_err(|e| format!("no se pudo restaurar «{p}»: {e}"))?;
            let _ = std::fs::remove_file(&item.info_file);
            items.retain(|it| it.info_file != item.info_file);
            Ok(())
        })
        .collect()
}

#[cfg(test)]
mod recycle_tests {
    use super::*;

    #[test]
    fn parses_v2_info_files() {
        let path = "C:\\datos\\ñu.txt";
        let units: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut b = Vec::new();
        b.extend_from_slice(&2u64.to_le_bytes());
        b.extend_from_slice(&1234u64.to_le_bytes());
        b.extend_from_slice(&116_444_736_000_000_000u64.to_le_bytes());
        b.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in units {
            b.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(parse_recycle_info(&b), Some((path.to_string(), 0, 1234)));
    }

    #[test]
    fn recycle_and_restore_round_trip() {
        let dir = std::env::temp_dir().join(format!("desvan-undo-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let f = dir.join("sub").join("vuelve.txt");
        std::fs::write(&f, b"hola de nuevo").unwrap();
        let p = f.to_string_lossy().into_owned();
        let t0 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        recycle(std::slice::from_ref(&p)).unwrap();
        assert!(!f.exists());
        let r = restore(std::slice::from_ref(&p), t0);
        assert!(r[0].is_ok(), "{r:?}");
        assert_eq!(std::fs::read(&f).unwrap(), b"hola de nuevo");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
