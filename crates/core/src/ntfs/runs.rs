//! Data runs: la lista de fragmentos (clúster lógico + longitud) de un atributo no residente.

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// Primer clúster virtual del fragmento dentro del atributo.
    pub vcn: u64,
    /// Clúster lógico en el volumen; `None` si el fragmento es disperso (sin datos en disco).
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Decodifica una lista de runs. `start_vcn` es el VCN inicial del atributo (0 en el primer tramo).
pub fn decode(mut b: &[u8], start_vcn: u64) -> Result<Vec<Run>> {
    let mut out = Vec::new();
    let mut lcn: i64 = 0;
    let mut vcn = start_vcn;
    while let Some(&h) = b.first() {
        if h == 0 {
            break;
        }
        let len_size = (h & 0x0F) as usize;
        let off_size = (h >> 4) as usize;
        if len_size == 0 || len_size > 8 || off_size > 8 || b.len() < 1 + len_size + off_size {
            return Err(Error::Corrupt("data run mal formado".into()));
        }
        let mut len: u64 = 0;
        for i in 0..len_size {
            len |= (b[1 + i] as u64) << (8 * i);
        }
        let run_lcn = if off_size == 0 {
            None
        } else {
            let mut off: i64 = 0;
            for i in 0..off_size {
                off |= (b[1 + len_size + i] as i64) << (8 * i);
            }
            // extensión de signo
            let shift = 64 - 8 * off_size as u32;
            off = (off << shift) >> shift;
            lcn = lcn.checked_add(off).ok_or_else(|| Error::Corrupt("LCN desbordado".into()))?;
            if lcn < 0 {
                return Err(Error::Corrupt("LCN negativo".into()));
            }
            Some(lcn as u64)
        };
        out.push(Run { vcn, lcn: run_lcn, clusters: len });
        vcn += len;
        b = &b[1 + len_size + off_size..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_relative_and_sparse_runs() {
        // 0x21: longitud 1 byte, desplazamiento 2 bytes
        // run 1: 0x18 clústeres en LCN 0x5634
        // run 2: disperso de 0x10 clústeres (cabecera 0x01)
        // run 3: 0x08 clústeres en LCN 0x5634 - 0x34 = 0x5600 (desplazamiento negativo 0xFFCC)
        let raw = [0x21, 0x18, 0x34, 0x56, 0x01, 0x10, 0x21, 0x08, 0xCC, 0xFF, 0x00];
        let r = decode(&raw, 0).unwrap();
        assert_eq!(r[0], Run { vcn: 0, lcn: Some(0x5634), clusters: 0x18 });
        assert_eq!(r[1], Run { vcn: 0x18, lcn: None, clusters: 0x10 });
        assert_eq!(r[2], Run { vcn: 0x28, lcn: Some(0x5600), clusters: 0x08 });
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(&[0x9F, 1, 2], 0).is_err());
        assert!(decode(&[0x21, 0x18], 0).is_err());
        assert!(decode(&[0x11, 0x01, 0x80], 0).is_err()); // LCN negativo
    }
}
