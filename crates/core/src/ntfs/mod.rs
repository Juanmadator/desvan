//! Lectura directa de NTFS: sector de arranque, data runs, registros FILE y escaneo de la MFT.

pub mod boot;
pub mod record;
pub mod runs;
pub mod scan;
pub mod source;

pub use scan::scan;
pub use source::{FileSource, MemSource, Source};
