//! Read-only GoldSrc BSP v30 header inventory; no map bytes are redistributed.
use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const LUMPS: usize = 15;
const HEADER_SIZE: usize = 4 + LUMPS * 8;
const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, PartialEq)]
struct Lump {
    offset: u64,
    length: u64,
}

fn parse_header(header: &[u8], file_size: u64) -> io::Result<Vec<Lump>> {
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

fn inspect(path: &Path) -> io::Result<Vec<Lump>> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut header = [0; HEADER_SIZE];
    file.read_exact(&mut header)?;
    parse_header(&header, size)
}

fn main() -> io::Result<()> {
    let mut args = env::args_os();
    let _program = args.next();
    let path = args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: bsp_inventory <local-map.bsp>",
        )
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one map path",
        ));
    }
    let lumps = inspect(Path::new(&path))?;
    println!("GoldSrc BSP version=30 lumps={}", lumps.len());
    for (index, lump) in lumps.iter().enumerate() {
        println!("lump={index} offset={} length={}", lump.offset, lump.length);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> [u8; HEADER_SIZE] {
        let mut data = [0; HEADER_SIZE];
        data[..4].copy_from_slice(&30_i32.to_le_bytes());
        data[4..8].copy_from_slice(&(HEADER_SIZE as i32).to_le_bytes());
        data[8..12].copy_from_slice(&4_i32.to_le_bytes());
        data
    }

    #[test]
    fn accepts_bounded_lumps_and_zero_length_entries() {
        let lumps = parse_header(&header(), (HEADER_SIZE + 4) as u64).unwrap();
        assert_eq!(lumps.len(), 15);
        assert_eq!(
            lumps[0],
            Lump {
                offset: HEADER_SIZE as u64,
                length: 4
            }
        );
        assert_eq!(
            lumps[1],
            Lump {
                offset: 0,
                length: 0
            }
        );
    }

    #[test]
    fn rejects_truncation_version_signed_overflow_and_out_of_bounds() {
        let original = header();
        assert!(parse_header(&original[..HEADER_SIZE - 1], 1000).is_err());
        assert!(parse_header(&original, 1).is_err());
        assert!(parse_header(&original, MAX_FILE_SIZE + 1).is_err());
        assert!(parse_header(&original, HEADER_SIZE as u64).is_err());
        for (offset, value) in [(0, 29_i32), (4, -1), (8, -1), (4, 4), (8, i32::MAX)] {
            let mut data = original;
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(parse_header(&data, 1000).is_err());
        }
    }

    #[test]
    fn reads_real_files_and_rejects_short_header() {
        let mut file = std::env::temp_dir();
        file.push(format!("goldsrc-bsp-test-{}", std::process::id()));
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .unwrap();
        use std::io::Write;
        output.write_all(&header()).unwrap();
        output.write_all(&[0; 4]).unwrap();
        drop(output);
        assert_eq!(inspect(&file).unwrap().len(), 15);
        std::fs::remove_file(&file).unwrap();
    }
}
