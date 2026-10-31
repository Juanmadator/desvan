//! Registros FILE de la MFT: fixups y atributos ($STANDARD_INFORMATION, $FILE_NAME, $DATA…).

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_INDEX_ALLOCATION: u32 = 0xA0;
pub const ATTR_BITMAP: u32 = 0xB0;
pub const ATTR_END: u32 = 0xFFFF_FFFF;

pub const REC_IN_USE: u16 = 0x0001;
pub const REC_DIRECTORY: u16 = 0x0002;

// atributos de archivo de Windows ($STANDARD_INFORMATION)
pub const FA_HIDDEN: u32 = 0x2;
pub const FA_SYSTEM: u32 = 0x4;
pub const FA_SPARSE: u32 = 0x200;
pub const FA_REPARSE: u32 = 0x400;
pub const FA_COMPRESSED: u32 = 0x800;
pub const FA_OFFLINE: u32 = 0x1000;
pub const FA_RECALL_ON_OPEN: u32 = 0x40000;
pub const FA_RECALL_ON_DATA_ACCESS: u32 = 0x400000;

#[inline]
pub fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
#[inline]
pub fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
#[inline]
pub fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Referencia de la MFT: los 48 bits bajos son el número de registro.
#[inline]
pub fn mft_ref(r: u64) -> u64 {
    r & 0x0000_FFFF_FFFF_FFFF
}

/// Aplica la "update sequence array": cada sector acaba con un número de control que
/// se sustituye por los bytes originales. Devuelve `false` si el registro está dañado
/// (escritura interrumpida) o no es un registro FILE.
pub fn apply_fixups(rec: &mut [u8]) -> bool {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return false;
    }
    let usa_off = u16le(rec, 4) as usize;
    let usa_count = u16le(rec, 6) as usize;
    if usa_count < 2 || usa_off + usa_count * 2 > rec.len() {
        return false;
    }
    let stride = rec.len() / (usa_count - 1);
    if stride < 256 || stride * (usa_count - 1) != rec.len() {
        return false;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * stride;
        if rec[end - 2] != usn[0] || rec[end - 1] != usn[1] {
            return false;
        }
        rec[end - 2] = rec[usa_off + 2 * i];
        rec[end - 1] = rec[usa_off + 2 * i + 1];
    }
    true
}

/// Un nombre ($FILE_NAME) de un registro.
#[derive(Clone, Copy, Debug)]
pub struct NameRef {
    pub parent: u64,
    pub namespace: u8,
    /// Posición del nombre UTF-16 dentro del registro y número de caracteres.
    pub off: usize,
    pub chars: usize,
}

/// Lo que nos interesa de un registro (sin asignar memoria: los nombres apuntan al búfer).
#[derive(Clone, Debug, Default)]
pub struct RecordInfo {
    pub in_use: bool,
    pub is_dir: bool,
    /// Registro base (0 si este es el base).
    pub base: u64,
    pub has_std: bool,
    pub std_attrs: u32,
    /// FILETIME de modificación.
    pub mtime: u64,
    pub has_attr_list: bool,
    /// $DATA sin nombre (primer tramo): tamaño lógico y en disco.
    pub data: Option<(u64, u64)>,
    /// Suma en disco de los flujos con nombre (ADS, WofCompressedData…).
    pub named_alloc: u64,
    pub named_streams: u32,
    /// Asignación del índice del directorio ($I30).
    pub index_alloc: u64,
    pub names_len: usize,
    pub names: [Option<NameRef>; 8],
    /// Más de 8 nombres (muy raro): los demás se pierden.
    pub names_overflow: bool,
}

impl RecordInfo {
    pub fn names(&self) -> impl Iterator<Item = &NameRef> {
        self.names[..self.names_len].iter().flatten()
    }
}

/// Clústeres con datos reales en un tramo de runs, o `None` si no hay huecos.
#[inline]
fn real_clusters(mut runs: &[u8]) -> Option<u64> {
    let mut total = 0u64;
    let mut holes = false;
    while let Some(&h) = runs.first() {
        if h == 0 {
            break;
        }
        let ls = (h & 0x0F) as usize;
        let os = (h >> 4) as usize;
        if ls == 0 || ls > 8 || os > 8 || runs.len() < 1 + ls + os {
            break;
        }
        let mut len = 0u64;
        for i in 0..ls {
            len |= (runs[1 + i] as u64) << (8 * i);
        }
        if os == 0 {
            holes = true;
        } else {
            total += len;
        }
        runs = &runs[1 + ls + os..];
    }
    holes.then_some(total)
}

/// Tamaño en disco de un atributo no residente: si es comprimido o disperso, los clústeres reales.
/// Algunos formateadores no marcan como disperso un atributo con huecos ($BadClus:$Bad), así que
/// si el runlist tiene huecos se cuentan solo los clústeres presentes.
#[inline]
fn nonresident_alloc(a: &[u8], attr_flags: u16, runlist_off: usize, cluster: u64) -> u64 {
    let allocated = u64le(a, 0x28);
    if attr_flags & (0x0001 | 0x8000) != 0 && runlist_off >= 0x48 && a.len() >= 0x48 {
        return u64le(a, 0x40);
    }
    if runlist_off < a.len() && cluster > 0 {
        if let Some(c) = real_clusters(&a[runlist_off..]) {
            return (c * cluster).min(allocated);
        }
    }
    allocated
}

/// Lee un registro ya corregido con [`apply_fixups`]. `cluster` es el tamaño de clúster del volumen.
pub fn parse(rec: &[u8], cluster: u64) -> RecordInfo {
    let mut info = RecordInfo::default();
    let rflags = u16le(rec, 0x16);
    info.in_use = rflags & REC_IN_USE != 0;
    info.is_dir = rflags & REC_DIRECTORY != 0;
    info.base = mft_ref(u64le(rec, 0x20));
    if !info.in_use {
        return info;
    }
    let used = (u32le(rec, 0x18) as usize).min(rec.len());
    let mut off = u16le(rec, 0x14) as usize;
    while off + 16 <= used {
        let ty = u32le(rec, off);
        if ty == ATTR_END {
            break;
        }
        let len = u32le(rec, off + 4) as usize;
        if len < 16 || off + len > used {
            break;
        }
        let a = &rec[off..off + len];
        let non_resident = a[8] != 0;
        let name_len = a[9] as usize;
        let attr_flags = u16le(a, 0x0C);
        match ty {
            ATTR_STANDARD_INFORMATION if !non_resident && len >= 0x18 => {
                let vo = u16le(a, 0x14) as usize;
                let vl = u32le(a, 0x10) as usize;
                if vo + 0x24 <= a.len() && vl >= 0x24 {
                    info.has_std = true;
                    info.mtime = u64le(a, vo + 0x08);
                    info.std_attrs = u32le(a, vo + 0x20);
                }
            }
            ATTR_ATTRIBUTE_LIST => info.has_attr_list = true,
            ATTR_FILE_NAME if !non_resident && len >= 0x18 => {
                let vo = u16le(a, 0x14) as usize;
                if vo + 0x42 <= a.len() {
                    let v = &a[vo..];
                    let chars = v[0x40] as usize;
                    let ns = v[0x41];
                    // los nombres DOS 8.3 (espacio 2) son duplicados del nombre largo
                    if ns != 2 && 0x42 + chars * 2 <= v.len() {
                        let nr = NameRef { parent: mft_ref(u64le(v, 0)), namespace: ns, off: off + vo + 0x42, chars };
                        if info.names_len < info.names.len() {
                            info.names[info.names_len] = Some(nr);
                            info.names_len += 1;
                        } else {
                            info.names_overflow = true;
                        }
                    }
                }
            }
            ATTR_DATA => {
                if non_resident {
                    if len >= 0x40 {
                        let start_vcn = u64le(a, 0x10);
                        if start_vcn == 0 {
                            let runlist_off = u16le(a, 0x20) as usize;
                            let size = u64le(a, 0x30);
                            let alloc = nonresident_alloc(a, attr_flags, runlist_off, cluster);
                            if name_len == 0 {
                                info.data = Some((size, alloc));
                            } else {
                                info.named_alloc += alloc;
                                info.named_streams += 1;
                            }
                        }
                    }
                } else if len >= 0x18 {
                    let vl = u32le(a, 0x10) as u64;
                    if name_len == 0 {
                        // residente: vive dentro del propio registro de la MFT, no ocupa clústeres
                        info.data = Some((vl, 0));
                    } else {
                        info.named_streams += 1;
                    }
                }
            }
            ATTR_INDEX_ALLOCATION if non_resident && len >= 0x40 && u64le(a, 0x10) == 0 => {
                info.index_alloc += u64le(a, 0x28);
            }
            _ => {}
        }
        off += len;
    }
    info
}

/// Convierte el nombre UTF-16 de un [`NameRef`] a texto.
pub fn name_string(rec: &[u8], n: &NameRef, out: &mut String) {
    out.clear();
    let units = (0..n.chars).map(|i| u16le(rec, n.off + i * 2));
    for ch in char::decode_utf16(units) {
        out.push(ch.unwrap_or('\u{FFFD}'));
    }
}

/// Busca un atributo no residente concreto (tipo y nombre vacío) y devuelve el
/// desplazamiento de su cabecera en el registro. Se usa para leer el $DATA y el $BITMAP de la propia $MFT.
pub fn find_attr(rec: &[u8], ty: u32, unnamed: bool) -> Vec<usize> {
    let mut v = Vec::new();
    let used = (u32le(rec, 0x18) as usize).min(rec.len());
    let mut off = u16le(rec, 0x14) as usize;
    while off + 16 <= used {
        let t = u32le(rec, off);
        if t == ATTR_END {
            break;
        }
        let len = u32le(rec, off + 4) as usize;
        if len < 16 || off + len > used {
            break;
        }
        if t == ty && (!unnamed || rec[off + 9] == 0) {
            v.push(off);
        }
        off += len;
    }
    v
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Constructor de registros FILE sintéticos para las pruebas.
    pub struct RecBuilder {
        pub buf: Vec<u8>,
        pub off: usize,
    }

    impl RecBuilder {
        pub fn new(size: usize, flags: u16, base: u64) -> Self {
            let mut buf = vec![0u8; size];
            buf[0..4].copy_from_slice(b"FILE");
            let usa_count = (size / 512 + 1) as u16;
            buf[4..6].copy_from_slice(&0x30u16.to_le_bytes());
            buf[6..8].copy_from_slice(&usa_count.to_le_bytes());
            let first_attr = (0x30 + usa_count as usize * 2 + 7) & !7;
            buf[0x14..0x16].copy_from_slice(&(first_attr as u16).to_le_bytes());
            buf[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
            buf[0x1C..0x20].copy_from_slice(&(size as u32).to_le_bytes());
            buf[0x20..0x28].copy_from_slice(&base.to_le_bytes());
            RecBuilder { buf, off: first_attr }
        }

        fn header(&mut self, ty: u32, len: usize, non_resident: bool, name: &str, flags: u16) -> usize {
            let o = self.off;
            let b = &mut self.buf;
            b[o..o + 4].copy_from_slice(&ty.to_le_bytes());
            b[o + 4..o + 8].copy_from_slice(&(len as u32).to_le_bytes());
            b[o + 8] = non_resident as u8;
            let units: Vec<u16> = name.encode_utf16().collect();
            b[o + 9] = units.len() as u8;
            b[o + 0x0C..o + 0x0E].copy_from_slice(&flags.to_le_bytes());
            o
        }

        pub fn resident(&mut self, ty: u32, name: &str, value: &[u8]) -> &mut Self {
            let units: Vec<u16> = name.encode_utf16().collect();
            let name_off = 0x18;
            let value_off = (name_off + units.len() * 2 + 7) & !7;
            let len = (value_off + value.len() + 7) & !7;
            let o = self.header(ty, len, false, name, 0);
            let b = &mut self.buf;
            b[o + 0x0A..o + 0x0C].copy_from_slice(&(name_off as u16).to_le_bytes());
            for (i, u) in units.iter().enumerate() {
                b[o + name_off + i * 2..o + name_off + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            b[o + 0x10..o + 0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
            b[o + 0x14..o + 0x16].copy_from_slice(&(value_off as u16).to_le_bytes());
            b[o + value_off..o + value_off + value.len()].copy_from_slice(value);
            self.off += len;
            self
        }

        #[allow(clippy::too_many_arguments)]
        pub fn nonresident(&mut self, ty: u32, name: &str, start_vcn: u64, alloc: u64, size: u64, flags: u16, compressed: Option<u64>, runs: &[u8]) -> &mut Self {
            let units: Vec<u16> = name.encode_utf16().collect();
            let hdr = if compressed.is_some() { 0x48 } else { 0x40 };
            let name_off = hdr;
            let runs_off = (name_off + units.len() * 2 + 7) & !7;
            let len = (runs_off + runs.len() + 7) & !7;
            let o = self.header(ty, len, true, name, flags);
            let b = &mut self.buf;
            b[o + 0x0A..o + 0x0C].copy_from_slice(&(name_off as u16).to_le_bytes());
            for (i, u) in units.iter().enumerate() {
                b[o + name_off + i * 2..o + name_off + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            b[o + 0x10..o + 0x18].copy_from_slice(&start_vcn.to_le_bytes());
            b[o + 0x20..o + 0x22].copy_from_slice(&(runs_off as u16).to_le_bytes());
            b[o + 0x28..o + 0x30].copy_from_slice(&alloc.to_le_bytes());
            b[o + 0x30..o + 0x38].copy_from_slice(&size.to_le_bytes());
            b[o + 0x38..o + 0x40].copy_from_slice(&size.to_le_bytes());
            if let Some(c) = compressed {
                b[o + 0x40..o + 0x48].copy_from_slice(&c.to_le_bytes());
            }
            b[o + runs_off..o + runs_off + runs.len()].copy_from_slice(runs);
            self.off += len;
            self
        }

        pub fn std_info(&mut self, mtime: u64, attrs: u32) -> &mut Self {
            let mut v = vec![0u8; 0x48];
            v[0x08..0x10].copy_from_slice(&mtime.to_le_bytes());
            v[0x20..0x24].copy_from_slice(&attrs.to_le_bytes());
            self.resident(ATTR_STANDARD_INFORMATION, "", &v)
        }

        pub fn file_name(&mut self, parent: u64, name: &str, ns: u8) -> &mut Self {
            let units: Vec<u16> = name.encode_utf16().collect();
            let mut v = vec![0u8; 0x42 + units.len() * 2];
            v[0..8].copy_from_slice(&(parent | (1u64 << 48)).to_le_bytes());
            v[0x40] = units.len() as u8;
            v[0x41] = ns;
            for (i, u) in units.iter().enumerate() {
                v[0x42 + i * 2..0x44 + i * 2].copy_from_slice(&u.to_le_bytes());
            }
            self.resident(ATTR_FILE_NAME, "", &v)
        }

        /// Cierra el registro y protege cada sector con la secuencia de actualización.
        pub fn finish(&mut self, usn: u16) -> Vec<u8> {
            let o = self.off;
            self.buf[o..o + 4].copy_from_slice(&ATTR_END.to_le_bytes());
            let used = (o + 8) as u32;
            self.buf[0x18..0x1C].copy_from_slice(&used.to_le_bytes());
            let mut b = self.buf.clone();
            let usa_off = 0x30;
            let count = b.len() / 512;
            b[usa_off..usa_off + 2].copy_from_slice(&usn.to_le_bytes());
            for i in 1..=count {
                let end = i * 512;
                let orig = [b[end - 2], b[end - 1]];
                b[usa_off + 2 * i..usa_off + 2 * i + 2].copy_from_slice(&orig);
                b[end - 2..end].copy_from_slice(&usn.to_le_bytes());
            }
            b
        }
    }

    #[test]
    fn fixups_restore_sector_tails() {
        let mut r = RecBuilder::new(1024, REC_IN_USE, 0);
        r.buf[510] = 0xAB;
        r.buf[511] = 0xCD;
        let mut rec = r.std_info(0, 0).finish(7);
        assert_eq!(&rec[510..512], &7u16.to_le_bytes());
        assert!(apply_fixups(&mut rec));
        assert_eq!(&rec[510..512], &[0xAB, 0xCD]);
        // un sector con número de control distinto = escritura rota
        let mut bad = RecBuilder::new(1024, REC_IN_USE, 0).std_info(0, 0).finish(7);
        bad[1022] ^= 0xFF;
        assert!(!apply_fixups(&mut bad));
    }

    #[test]
    fn parses_names_data_and_streams() {
        let mut rec = RecBuilder::new(1024, REC_IN_USE, 0)
            .std_info(132_000_000_000_000_000, FA_HIDDEN)
            .file_name(5, "INFORM~1.DOC", 2)
            .file_name(5, "Informe final.docx", 1)
            .file_name(40, "copia.docx", 0)
            .nonresident(ATTR_DATA, "", 0, 12288, 10_000, 0, None, &[0x11, 3, 0x20, 0])
            .nonresident(ATTR_DATA, "Zone.Identifier", 0, 4096, 26, 0, None, &[0x11, 1, 0x30, 0])
            .finish(1);
        assert!(apply_fixups(&mut rec));
        let info = parse(&rec, 4096);
        assert!(info.in_use && !info.is_dir);
        assert_eq!(info.std_attrs, FA_HIDDEN);
        assert_eq!(info.data, Some((10_000, 12288)));
        assert_eq!(info.named_alloc, 4096);
        assert_eq!(info.named_streams, 1);
        let names: Vec<(u64, String)> = info
            .names()
            .map(|n| {
                let mut s = String::new();
                name_string(&rec, n, &mut s);
                (n.parent, s)
            })
            .collect();
        assert_eq!(names, vec![(5, "Informe final.docx".to_string()), (40, "copia.docx".to_string())]);
    }

    #[test]
    fn sparse_and_compressed_use_real_clusters() {
        let mut rec = RecBuilder::new(1024, REC_IN_USE, 0)
            .std_info(0, FA_SPARSE)
            .file_name(5, "disco.vhdx", 1)
            .nonresident(ATTR_DATA, "", 0, 1 << 30, 1 << 30, 0x8000, Some(65536), &[0x01, 0x40, 0x11, 0x10, 0x05, 0])
            .finish(3);
        assert!(apply_fixups(&mut rec));
        assert_eq!(parse(&rec, 4096).data, Some((1 << 30, 65536)));
    }

    #[test]
    fn unflagged_holes_count_only_real_clusters() {
        // como $BadClus:$Bad de mkntfs: 1 GB lógico, un único run disperso y sin marca de disperso
        let mut rec = RecBuilder::new(1024, REC_IN_USE, 0)
            .file_name(5, "$BadClus", 3)
            .nonresident(ATTR_DATA, "", 0, 1 << 30, 1 << 30, 0, None, &[0x03, 0x00, 0x00, 0x04, 0])
            .finish(5);
        assert!(apply_fixups(&mut rec));
        assert_eq!(parse(&rec, 4096).data, Some((1 << 30, 0)));
        // huecos y datos mezclados: 3 clústeres reales
        let mut mixed = RecBuilder::new(1024, REC_IN_USE, 0)
            .file_name(5, "mixto.bin", 1)
            .nonresident(ATTR_DATA, "", 0, 40960, 40960, 0, None, &[0x11, 0x02, 0x10, 0x01, 0x05, 0x11, 0x01, 0x20, 0])
            .finish(5);
        assert!(apply_fixups(&mut mixed));
        assert_eq!(parse(&mixed, 4096).data, Some((40960, 3 * 4096)));
    }

    #[test]
    fn resident_data_takes_no_clusters() {
        let mut rec = RecBuilder::new(1024, REC_IN_USE, 0).file_name(5, "nota.txt", 3).resident(ATTR_DATA, "", b"hola").finish(9);
        assert!(apply_fixups(&mut rec));
        assert_eq!(parse(&rec, 4096).data, Some((4, 0)));
    }

    #[test]
    fn extension_records_point_to_base() {
        let mut rec = RecBuilder::new(1024, REC_IN_USE, 1234 | (3 << 48)).nonresident(ATTR_DATA, "", 0, 8192, 5000, 0, None, &[0x11, 2, 0x40, 0]).finish(2);
        assert!(apply_fixups(&mut rec));
        let info = parse(&rec, 4096);
        assert_eq!(info.base, 1234);
        assert_eq!(info.data, Some((5000, 8192)));
        // los tramos posteriores (VCN > 0) no traen tamaños válidos
        let mut later = RecBuilder::new(1024, REC_IN_USE, 1234).nonresident(ATTR_DATA, "", 2, 0, 0, 0, None, &[0x11, 2, 0x50, 0]).finish(2);
        assert!(apply_fixups(&mut later));
        assert_eq!(parse(&later, 4096).data, None);
    }

    #[test]
    fn deleted_records_are_skipped() {
        let mut rec = RecBuilder::new(1024, 0, 0).file_name(5, "borrado.txt", 1).finish(4);
        assert!(apply_fixups(&mut rec));
        let info = parse(&rec, 4096);
        assert!(!info.in_use);
        assert_eq!(info.names().count(), 0);
    }
}
