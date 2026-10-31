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
