//! Read-only GoldSrc BSP v30 metadata and multiplayer map CRC.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub const LUMPS: usize = 15;
pub const HEADER_SIZE: usize = 4 + LUMPS * 8;
pub const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct Lump {
    pub offset: u64,
    pub length: u64,
}

pub fn parse_header(header: &[u8], file_size: u64) -> io::Result<Vec<Lump>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid GoldSrc BSP v30 header");
    if header.len() != HEADER_SIZE || !(HEADER_SIZE as u64..=MAX_FILE_SIZE).contains(&file_size) {
        return Err(invalid());
    }
    if i32::from_le_bytes(header[..4].try_into().map_err(|_| invalid())?) != 30 {
        return Err(invalid());
    }
    let mut lumps = Vec::with_capacity(LUMPS);
    for entry in header[4..].as_chunks::<8>().0 {
        let offset = i32::from_le_bytes(entry[..4].try_into().map_err(|_| invalid())?);
        let length = i32::from_le_bytes(entry[4..].try_into().map_err(|_| invalid())?);
        if offset < 0 || length < 0 {
            return Err(invalid());
        }
        let offset = offset as u64;
        let length = length as u64;
        if offset.checked_add(length).is_none_or(|end| end > file_size)
            || (length != 0 && offset < HEADER_SIZE as u64)
        {
            return Err(invalid());
        }
        lumps.push(Lump { offset, length });
    }
    Ok(lumps)
}

/// HLDS multiplayer world CRC, before per-player transformation.
pub fn map_crc(reader: &mut (impl Read + Seek), file_size: u64) -> io::Result<(Vec<Lump>, u32)> {
    let mut header = [0; HEADER_SIZE];
    reader.seek(SeekFrom::Start(0))?;
    reader.read_exact(&mut header)?;
    let lumps = parse_header(&header, file_size)?;
    let mut crc = crc32fast::Hasher::new();
    let mut buffer = [0; 8192];
    for lump in lumps.iter().skip(1) {
        reader.seek(SeekFrom::Start(lump.offset))?;
        let mut remaining = lump.length;
        while remaining > 0 {
            let amount = remaining.min(buffer.len() as u64) as usize;
            reader.read_exact(&mut buffer[..amount])?;
            crc.update(&buffer[..amount]);
            remaining -= amount as u64;
        }
    }
    Ok((lumps, !crc.finalize()))
}

pub fn inspect(path: &Path) -> io::Result<(Vec<Lump>, u32)> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "map is not a regular file",
        ));
    }
    map_crc(&mut file, metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_directories_and_missing_files() {
        assert!(inspect(Path::new(env!("CARGO_MANIFEST_DIR"))).is_err());
        assert!(
            inspect(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("missing-map.bsp")
                    .as_path()
            )
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires local HLDS map"]
    fn real_map_crc_matches_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let (lumps, crc) = inspect(&path).unwrap();
        assert_eq!(lumps.len(), 15);
        assert_eq!(crc, 0x451b6da9);
    }
}
