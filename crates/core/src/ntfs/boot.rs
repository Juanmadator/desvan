//! Sector de arranque de NTFS: geometría del volumen y posición de la MFT.

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Boot {
    pub bytes_per_sector: u32,
    pub cluster: u64,
    pub total_sectors: u64,
    pub mft_lcn: u64,
    pub record_size: u32,
    pub serial: u64,
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

impl Boot {
    pub fn parse(b: &[u8]) -> Result<Boot> {
        if b.len() < 512 || &b[3..11] != b"NTFS    " {
            return Err(Error::NotNtfs);
        }
        let bytes_per_sector = u16le(b, 0x0B) as u32;
        if !(256..=4096).contains(&bytes_per_sector) || !bytes_per_sector.is_power_of_two() {
            return Err(Error::Corrupt(format!("bytes por sector: {bytes_per_sector}")));
        }
        // 0x0D: sectores por clúster; a partir de 128 se codifica como potencia negativa (clústeres > 64 KB)
        let spc_raw = b[0x0D] as u32;
        let spc = if spc_raw <= 0x80 { spc_raw } else { 1u32 << (256 - spc_raw) };
        if spc == 0 {
            return Err(Error::Corrupt("sectores por clúster = 0".into()));
        }
        let cluster = bytes_per_sector as u64 * spc as u64;
        let total_sectors = u64le(b, 0x28);
        let mft_lcn = u64le(b, 0x30);
        // 0x40: clústeres por registro de la MFT; si es negativo, el tamaño es 2^|n| bytes
        let cpr = b[0x40] as i8;
        let record_size = if cpr > 0 { (cpr as u64 * cluster) as u32 } else { 1u32 << (-(cpr as i32)) as u32 };
        if !(512..=65536).contains(&record_size) || !record_size.is_power_of_two() {
            return Err(Error::Corrupt(format!("tamaño de registro: {record_size}")));
        }
        Ok(Boot { bytes_per_sector, cluster, total_sectors, mft_lcn, record_size, serial: u64le(b, 0x48) })
    }

    pub fn volume_bytes(&self) -> u64 {
        self.total_sectors * self.bytes_per_sector as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fake_boot(cluster_sectors: u8, cpr: i8) -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        b[0x0D] = cluster_sectors;
        b[0x28..0x30].copy_from_slice(&2_000_000u64.to_le_bytes());
        b[0x30..0x38].copy_from_slice(&786_432u64.to_le_bytes());
        b[0x40] = cpr as u8;
        b
    }

    #[test]
    fn parses_standard_geometry() {
        let b = Boot::parse(&fake_boot(8, -10)).unwrap();
        assert_eq!(b.cluster, 4096);
        assert_eq!(b.record_size, 1024);
        assert_eq!(b.mft_lcn, 786_432);
        assert_eq!(b.volume_bytes(), 2_000_000 * 512);
    }

    #[test]
    fn big_clusters_and_records() {
        // 0xF4 => 2^(256-244) = 4096 sectores = 2 MB por clúster
        let b = Boot::parse(&fake_boot(0xF4, -12)).unwrap();
        assert_eq!(b.cluster, 2 * 1024 * 1024);
        assert_eq!(b.record_size, 4096);
    }

    #[test]
    fn rejects_other_filesystems() {
        let mut b = fake_boot(8, -10);
        b[3..11].copy_from_slice(b"EXFAT   ");
        assert!(matches!(Boot::parse(&b), Err(Error::NotNtfs)));
    }
}
