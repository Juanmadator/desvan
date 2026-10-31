//! Estado compartido: el escaneo actual y los trabajos en marcha.

use desvan_core::{Progress, Scan};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

pub struct ScanData {
    pub id: u32,
    /// Se modifica al borrar (se restan tamaños), por eso va con RwLock.
    pub scan: RwLock<Scan>,
    /// Categoría de archivo por nodo (para colores y estadísticas).
    pub cats: RwLock<Vec<u8>>,
    /// Nombres en minúsculas para buscar rápido (se calcula la primera vez).
    pub lower: std::sync::OnceLock<Vec<u8>>,
}

#[derive(Default)]
pub struct AppState {
    pub current: RwLock<Option<Arc<ScanData>>>,
    pub progress: Mutex<Option<Arc<Progress>>>,
    pub next_id: Mutex<u32>,
    pub dupes_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl AppState {
    pub fn current(&self) -> Result<Arc<ScanData>, String> {
        self.current.read().unwrap().clone().ok_or_else(|| "no hay ningún escaneo".to_string())
    }
}
