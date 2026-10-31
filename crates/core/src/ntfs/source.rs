//! Origen de los bytes: una imagen de disco o un volumen real (`\\.\C:`).
//!
//! Windows serializa las lecturas síncronas sobre un mismo handle, así que para leer
//! en paralelo de verdad se abren varios handles y cada hilo usa el suyo.

use std::fs::File;
use std::io;
use std::path::Path;

/// Lectura posicional, segura desde varios hilos a la vez.
pub trait Source: Send + Sync {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    /// Alineación que exigen las lecturas (búfer, desplazamiento y longitud). 1 = ninguna.
    fn alignment(&self) -> usize {
        1
    }
}

pub struct FileSource {
    handles: Vec<File>,
    align: usize,
}

/// Cuántos handles abrir (uno por hilo de E/S como mucho).
const HANDLES: usize = 8;

impl FileSource {
    /// Imagen de un volumen NTFS (por ejemplo, creada con `mkntfs`).
    pub fn open_image(path: &Path) -> io::Result<Self> {
        let mut handles = Vec::with_capacity(HANDLES);
        for _ in 0..HANDLES {
            handles.push(File::open(path)?);
        }
        Ok(Self { handles, align: 1 })
    }

    /// Volumen montado, por letra (necesita permisos de administrador).
    #[cfg(windows)]
    pub fn open_volume(letter: char) -> crate::Result<Self> {
        Self::open_volume_with(letter, false)
    }

    /// `unbuffered`: lee sin pasar por la caché de Windows (DMA directo a nuestros búferes,
    /// que deben estar alineados al sector). No ensucia la caché del sistema.
    #[cfg(windows)]
    pub fn open_volume_with(letter: char, unbuffered: bool) -> crate::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
        let path = format!(r"\\.\{}:", letter.to_ascii_uppercase());
        let mut handles = Vec::with_capacity(HANDLES);
        for _ in 0..HANDLES {
            let mut o = std::fs::OpenOptions::new();
            o.read(true).share_mode(0x1 | 0x2 | 0x4);
            o.custom_flags(if unbuffered { FILE_FLAG_NO_BUFFERING } else { FILE_FLAG_SEQUENTIAL_SCAN });
            match o.open(&path) {
                Ok(f) => handles.push(f),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5) => return Err(crate::Error::AccessDenied),
                Err(e) => return Err(e.into()),
            }
        }
        // 4096 cubre sectores de 512 y de 4K
        Ok(Self { handles, align: if unbuffered { 4096 } else { 1 } })
    }

    fn handle(&self) -> &File {
        let i = rayon::current_thread_index().unwrap_or(0) % self.handles.len();
        &self.handles[i]
    }
}

impl Source for FileSource {
    fn read_at(&self, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
        let f = self.handle();
        while !buf.is_empty() {
            #[cfg(windows)]
            let n = std::os::windows::fs::FileExt::seek_read(f, buf, offset)?;
            #[cfg(unix)]
            let n = std::os::unix::fs::FileExt::read_at(f, buf, offset)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "fin del volumen"));
            }
            offset += n as u64;
            buf = &mut buf[n..];
        }
        Ok(())
    }

    fn alignment(&self) -> usize {
        self.align
    }
}

/// Búfer con la memoria alineada (para lecturas sin caché).
pub struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: std::alloc::Layout,
}

unsafe impl Send for AlignedBuf {}

impl AlignedBuf {
    pub fn new(len: usize, align: usize) -> Self {
        let align = align.max(16);
        let cap = len.div_ceil(align) * align;
        let layout = std::alloc::Layout::from_size_align(cap.max(align), align).expect("alineación");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Self { ptr, len, layout }
    }
}

impl std::ops::Deref for AlignedBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl std::ops::DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.ptr, self.layout) }
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
