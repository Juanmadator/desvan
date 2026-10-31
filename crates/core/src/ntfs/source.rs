//! Origen de los bytes: una imagen de disco o un volumen real (`\\.\C:`).

use std::fs::File;
use std::io;
use std::path::Path;

/// Lectura posicional, segura desde varios hilos a la vez.
pub trait Source: Send + Sync {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

pub struct FileSource {
    file: File,
}

impl FileSource {
    /// Imagen de un volumen NTFS (por ejemplo, creada con `mkntfs`).
    pub fn open_image(path: &Path) -> io::Result<Self> {
        Ok(Self { file: File::open(path)? })
    }

    /// Volumen montado, por letra (necesita permisos de administrador).
    #[cfg(windows)]
    pub fn open_volume(letter: char) -> crate::Result<Self> {
        let path = format!(r"\\.\{}:", letter.to_ascii_uppercase());
        match File::open(&path) {
            Ok(file) => Ok(Self { file }),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5) => Err(crate::Error::AccessDenied),
            Err(e) => Err(e.into()),
        }
    }
}

impl Source for FileSource {
    fn read_at(&self, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            #[cfg(windows)]
            let n = std::os::windows::fs::FileExt::seek_read(&self.file, buf, offset)?;
            #[cfg(unix)]
            let n = std::os::unix::fs::FileExt::read_at(&self.file, buf, offset)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "fin del volumen"));
            }
            offset += n as u64;
            buf = &mut buf[n..];
        }
        Ok(())
    }
}

/// Origen en memoria (pruebas).
pub struct MemSource(pub Vec<u8>);

impl Source for MemSource {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let o = offset as usize;
        if o + buf.len() > self.0.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "fuera de rango"));
        }
        buf.copy_from_slice(&self.0[o..o + buf.len()]);
        Ok(())
    }
}
