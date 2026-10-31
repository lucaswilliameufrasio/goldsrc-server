//! Read-only GoldSrc BSP v30 header inventory; no map bytes are redistributed.
use std::env;
use std::io;
use std::path::Path;
use steam_server_query_poc::map::inspect;
#[cfg(test)]
use steam_server_query_poc::map::{HEADER_SIZE, Lump, MAX_FILE_SIZE, map_crc, parse_header};

#[path = "../src/serverinfo.rs"]
mod serverinfo;

fn main() -> io::Result<()> {
    let mut args = env::args_os();
    let _program = args.next();
    let path = args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: bsp_inventory <local-map.bsp> [--serverinfo-slot <0..31>]",
        )
    })?;
    let slot = match args.next() {
        None => None,
        Some(flag) if flag == "--serverinfo-slot" => {
            let raw = args
                .next()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing slot"))?;
            let slot = raw
                .to_str()
                .and_then(|s| s.parse::<u8>().ok())
                .filter(|value| *value < 32)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "slot must be 0..31"))?;
            if args.next().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unexpected argument",
                ));
            }
            Some(slot)
        }
        Some(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown option",
            ));
        }
    };
    let (lumps, crc) = inspect(Path::new(&path))?;
    println!("GoldSrc BSP version=30 lumps={}", lumps.len());
    println!("GoldSrc CS map CRC32=0x{crc:08x}");
    for (index, lump) in lumps.iter().enumerate() {
        println!("lump={index} offset={} length={}", lump.offset, lump.length);
    }
    if let Some(slot) = slot {
        let name = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| {
                s.ends_with(".bsp")
                    && s.len() <= 64
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "map name must be a safe ASCII .bsp filename",
                )
            })?;
        let model = format!("maps/{name}");
        let payload = serverinfo::ServerInfo {
            spawn_count: 1,
            world_crc: crc,
            client_dll_md5: [0; 16],
            max_players: 32,
            player_index: slot,
            game_type: 1,
            game_dir: "cstrike",
            hostname: "Diagnostic only",
            world_model: &model,
            map_cycle: "",
            secure: false,
        }
        .encode()?;
        print!("synthetic serverinfo (not sent; placeholder spawn/hostname) hex=");
        for byte in payload {
            print!("{byte:02x}");
        }
        println!();
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
