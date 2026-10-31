//! Desván: análisis del espacio en disco.
//!
//! - [`ntfs`]: lector directo de la MFT de un volumen NTFS (el método rápido).
//! - [`tree`]: árbol compacto común a todos los escáneres.

pub mod categories;
pub mod dupes;
pub mod junk;
pub mod layout;
pub mod ntfs;
pub mod snapshot;
pub mod tree;
pub mod util;
pub mod walk;
#[cfg(windows)]
pub mod helper;
#[cfg(windows)]
pub mod win;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::Duration;

pub use tree::{flags, Builder, NodeId, Tree, NONE};

/// Cómo se obtuvo el árbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Lectura directa de la tabla maestra de archivos de NTFS.
    Mft,
    /// Recorrido de carpetas en paralelo (cualquier sistema de archivos, sin permisos de administrador).
    Walk,
}

#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct ScanStats {
    pub elapsed_ms: u64,
    /// Tiempo leyendo del disco (solo MFT).
    pub read_ms: u64,
    /// Tiempo construyendo el árbol.
    pub build_ms: u64,
    pub bytes_read: u64,
    /// Registros de la MFT examinados.
    pub records: u64,
    pub files: u64,
    pub dirs: u64,
    /// Registros dañados o ilegibles que se saltaron.
    pub bad_records: u64,
    /// Carpetas que no se pudieron abrir (recorrido).
    pub denied: u64,
    pub threads: usize,
}

/// Información del volumen escaneado.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct VolumeInfo {
    pub total: u64,
    pub free: u64,
    pub cluster: u32,
    pub filesystem: String,
    pub label: String,
}

pub struct Scan {
    pub tree: Tree,
    /// Ruta de la raíz (p. ej. `C:\`).
    pub root_path: String,
    pub method: Method,
    pub stats: ScanStats,
    pub volume: Option<VolumeInfo>,
}

/// Fases del escaneo, para la barra de progreso.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Starting = 0,
    Reading = 1,
    Building = 2,
    Done = 3,
}

/// Progreso compartido entre el hilo del escaneo y la interfaz (solo atómicos: sin bloqueos).
#[derive(Default)]
pub struct Progress {
    pub phase: AtomicU8,
    /// Unidades hechas / totales (registros de MFT o carpetas).
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn set_phase(&self, p: Phase) {
        self.phase.store(p as u8, Ordering::Relaxed);
    }
    pub fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Relaxed) {
            1 => Phase::Reading,
            2 => Phase::Building,
            3 => Phase::Done,
            _ => Phase::Starting,
        }
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            phase: self.phase(),
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProgressSnapshot {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no es un volumen NTFS")]
    NotNtfs,
    #[error("hacen falta permisos de administrador para leer la MFT")]
    AccessDenied,
    #[error("estructura NTFS no válida: {0}")]
    Corrupt(String),
    #[error("escaneo cancelado")]
    Cancelled,
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}
