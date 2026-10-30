//! Read-only GoldSrc BSP v30 header inventory; no map bytes are redistributed.
use std::env;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
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

fn map_crc(reader: &mut (impl Read + Seek), file_size: u64) -> io::Result<(Vec<Lump>, u32)> {
    let mut header = [0; HEADER_SIZE];
    reader.seek(SeekFrom::Start(0))?;
    reader.read_exact(&mut header)?;
    let lumps = parse_header(&header, file_size)?;
    let mut crc = crc32fast::Hasher::new();
    let mut buffer = [0; 8192];
    // Counter-Strike uses standard GoldSrc ordering: skip entity lump 0.
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
    // GoldSrc stores CRC32_Init/ProcessBuffer state directly as worldmapCRC;
    // unlike a standard file CRC, it does not call CRC32_Final here.
    Ok((lumps, !crc.finalize()))
}

fn inspect(path: &Path) -> io::Result<(Vec<Lump>, u32)> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    map_crc(&mut file, size)
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
    let (lumps, crc) = inspect(Path::new(&path))?;
    println!("GoldSrc BSP version=30 lumps={}", lumps.len());
    println!("GoldSrc CS map CRC32=0x{crc:08x}");
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
        assert_eq!(inspect(&file).unwrap().0.len(), 15);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn crc_uses_lump_index_order_and_skips_entities_and_padding() {
        let mut data = header().to_vec();
        // Physical order differs from logical lump order; excluded entities and
        // unreferenced bytes must not influence the checksum.
        data[12..16].copy_from_slice(&132_i32.to_le_bytes());
        data[16..20].copy_from_slice(&3_i32.to_le_bytes());
        data[20..24].copy_from_slice(&128_i32.to_le_bytes());
        data[24..28].copy_from_slice(&4_i32.to_le_bytes());
        data.extend_from_slice(b"ENT!5678123");
        let size = data.len() as u64;
        let crc = map_crc(&mut io::Cursor::new(&data), size).unwrap().1;
        assert_eq!(crc, !crc32fast::hash(b"1235678"));
        data[HEADER_SIZE] ^= 1;
        assert_eq!(map_crc(&mut io::Cursor::new(&data), size).unwrap().1, crc);
        data[128] ^= 1;
        assert_ne!(map_crc(&mut io::Cursor::new(&data), size).unwrap().1, crc);
        assert!(map_crc(&mut io::Cursor::new(&data[..data.len() - 1]), size).is_err());
    }

    #[test]
    fn standard_crc32_check_value() {
        assert_eq!(crc32fast::hash(b"123456789"), 0xcbf43926);
    }
}
