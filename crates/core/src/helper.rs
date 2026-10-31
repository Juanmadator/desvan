//! Escaneo MFT a través de un ayudante elevado.
//!
//! La interfaz no necesita permisos de administrador: cuando hay que leer la MFT lanza
//! su propio ejecutable con `--mft-helper <letra> <tubería>` y el verbo `runas` (sin ventana).
//! El ayudante escribe por una tubería con nombre tramas `[tipo u8][longitud u64][datos]`:
//! progreso cada 100 ms y, al final, el escaneo serializado (o un error).

use crate::{snapshot, Error, Phase, Progress, Result, Scan};
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_GENERIC_WRITE, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_INBOUND, FILE_FLAGS_AND_ATTRIBUTES};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows::Win32::System::Threading::{GetExitCodeProcess, TerminateProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const T_PROGRESS: u8 = 1;
const T_RESULT: u8 = 2;
const T_ERROR: u8 = 3;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn frame<W: Write>(w: &mut W, ty: u8, data: &[u8]) -> std::io::Result<()> {
    w.write_all(&[ty])?;
    w.write_all(&(data.len() as u64).to_le_bytes())?;
    w.write_all(data)?;
    w.flush()
}

// ---------- lado del ayudante (elevado) ----------

/// Punto de entrada del ayudante: conecta con la tubería, escanea y envía el resultado.
/// Devuelve el código de salida del proceso.
pub fn serve(letter: char, pipe: &str) -> i32 {
    let wpipe = wide(pipe);
    let h = unsafe { CreateFileW(PCWSTR(wpipe.as_ptr()), FILE_GENERIC_WRITE.0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) };
    let Ok(h) = h else { return 2 };
    let out = Arc::new(Mutex::new(unsafe { File::from_raw_handle(h.0 as _) }));
    let progress = Arc::new(Progress::default());
    let done = Arc::new(AtomicBool::new(false));
    // progreso periódico; si la interfaz se va (tubería rota), se cancela el escaneo
    let ticker = {
        let (out, progress, done) = (out.clone(), progress.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let s = progress.snapshot();
                let mut b = Vec::with_capacity(40);
                for v in [s.phase as u64, s.done, s.total, s.files, s.bytes] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                if frame(&mut *out.lock().unwrap(), T_PROGRESS, &b).is_err() {
                    progress.cancel.store(true, Ordering::Relaxed);
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let root = format!("{}:\\", letter.to_ascii_uppercase());
    let result = crate::ntfs::FileSource::open_volume(letter).and_then(|src| crate::ntfs::scan(&src, &root, &progress));
    done.store(true, Ordering::Relaxed);
    let _ = ticker.join();
    let mut w = out.lock().unwrap();
    match result {
        Ok(scan) => {
            let mut buf = Vec::new();
            if snapshot::write_scan(&mut buf, &scan).is_err() || frame(&mut *w, T_RESULT, &buf).is_err() {
                return 3;
            }
            0
        }
        Err(e) => {
            let _ = frame(&mut *w, T_ERROR, e.to_string().as_bytes());
            1
        }
    }
}

// ---------- lado de la interfaz ----------

struct OwnedHandle(HANDLE);
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Escanea `letter` con la MFT lanzando `exe --mft-helper` elevado. Si el usuario rechaza
/// el aviso de permisos devuelve [`Error::AccessDenied`] (para recurrir al recorrido normal).
pub fn scan_elevated(exe: &std::path::Path, letter: char, progress: &Progress) -> Result<Scan> {
    let pipe = format!(r"\\.\pipe\desvan-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    let wpipe = wide(&pipe);
    let server = unsafe { CreateNamedPipeW(PCWSTR(wpipe.as_ptr()), PIPE_ACCESS_INBOUND, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 0, 1 << 20, 0, None) };
    if server == INVALID_HANDLE_VALUE {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }

    // lanzar el ayudante elevado y sin ventana
    let wexe = wide(&exe.to_string_lossy());
    let wargs = wide(&format!("--mft-helper {} {}", letter.to_ascii_uppercase(), pipe));
    let verb = wide("runas");
    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(wexe.as_ptr()),
        lpParameters: PCWSTR(wargs.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut sei) }.is_err() {
        let cancelled = unsafe { GetLastError() } == ERROR_CANCELLED;
        unsafe {
            let _ = CloseHandle(server);
        }
        return Err(if cancelled { Error::AccessDenied } else { Error::Io(std::io::Error::last_os_error()) });
    }
    let proc = Arc::new(OwnedHandle(sei.hProcess));

    // si el ayudante muere sin conectar, nos conectamos a nosotros mismos para desbloquear la espera
    let connected = Arc::new(AtomicBool::new(false));
    {
        let (proc, connected, pipe) = (proc.clone(), connected.clone(), pipe.clone());
        std::thread::spawn(move || {
            loop {
                if connected.load(Ordering::Relaxed) {
                    return;
                }
                if unsafe { WaitForSingleObject(proc.0, 200) }.0 == 0 {
                    // terminó: si no llegó a conectar, abrimos la tubería como cliente falso
                    if !connected.load(Ordering::Relaxed) {
                        let w = wide(&pipe);
                        if let Ok(h) = unsafe { CreateFileW(PCWSTR(w.as_ptr()), FILE_GENERIC_WRITE.0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) } {
                            unsafe {
                                let _ = CloseHandle(h);
                            }
                        }
                    }
                    return;
                }
            }
        });
    }
    let ok = unsafe { ConnectNamedPipe(server, None) };
    if ok.is_err() && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
        unsafe {
            let _ = CloseHandle(server);
        }
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    connected.store(true, Ordering::Relaxed);
    let mut pipe_file = unsafe { File::from_raw_handle(server.0 as _) };

    // vigía de cancelación: si el usuario cancela, se termina el ayudante (y se rompe la tubería)
    let finished = Arc::new(AtomicBool::new(false));
    {
        let (proc, finished) = (proc.clone(), finished.clone());
        let cancel_flag: &'static AtomicBool = unsafe { &*(&progress.cancel as *const AtomicBool) };
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                if cancel_flag.load(Ordering::Relaxed) {
                    unsafe {
                        let _ = TerminateProcess(proc.0, 1);
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
    }

    let res = (|| -> Result<Scan> {
        loop {
            let mut head = [0u8; 9];
            if let Err(e) = pipe_file.read_exact(&mut head) {
                if progress.cancelled() {
                    return Err(Error::Cancelled);
                }
                let mut code = 0u32;
                unsafe {
                    let _ = GetExitCodeProcess(proc.0, &mut code);
                }
                return Err(Error::Io(std::io::Error::new(e.kind(), format!("el ayudante terminó sin resultado (código {code})"))));
            }
            let len = u64::from_le_bytes(head[1..9].try_into().unwrap()) as usize;
            match head[0] {
                T_PROGRESS => {
                    let mut b = vec![0u8; len];
                    pipe_file.read_exact(&mut b)?;
                    if b.len() >= 40 {
                        let v = |i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
                        progress.set_phase(match v(0) {
                            1 => Phase::Reading,
                            2 => Phase::Building,
                            _ => Phase::Starting,
                        });
                        progress.done.store(v(1), Ordering::Relaxed);
                        progress.total.store(v(2), Ordering::Relaxed);
                        progress.files.store(v(3), Ordering::Relaxed);
                        progress.bytes.store(v(4), Ordering::Relaxed);
                    }
                }
                T_RESULT => {
                    progress.set_phase(Phase::Building);
                    let mut limited = (&mut pipe_file).take(len as u64);
                    return Ok(snapshot::read_scan(&mut limited)?);
                }
                T_ERROR => {
                    let mut b = vec![0u8; len];
                    pipe_file.read_exact(&mut b)?;
                    let msg = String::from_utf8_lossy(&b).into_owned();
                    return Err(if msg.contains("administrador") { Error::AccessDenied } else { Error::Corrupt(msg) });
                }
                _ => return Err(Error::Corrupt("trama desconocida del ayudante".into())),
            }
        }
    })();
    finished.store(true, Ordering::Relaxed);
    res
}
