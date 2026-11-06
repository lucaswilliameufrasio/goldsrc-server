//! Read-only GoldSrc BSP v30 metadata and multiplayer map CRC.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub const LUMPS: usize = 15;
pub const HEADER_SIZE: usize = 4 + LUMPS * 8;
pub const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_ENTITY_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENTITIES: usize = 16_384;
const MAX_FIELDS: usize = 256;
const MAX_TOKEN: usize = 1024;

/// Raw BSP entity fields; bytes are retained because GoldSrc maps need not be UTF-8.
#[derive(Debug, PartialEq, Eq)]
pub struct MapEntity {
    pub fields: Vec<(Vec<u8>, Vec<u8>)>,
}

fn invalid_entities() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid BSP entity lump")
}

struct EntityParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl EntityParser<'_> {
    fn whitespace(&mut self) {
        loop {
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.input.get(self.pos..self.pos + 2) != Some(b"//") {
                break;
            }
            while self.pos < self.input.len() && self.input[self.pos] != b'\n' {
                self.pos += 1;
            }
        }
    }

    fn quoted(&mut self) -> io::Result<Vec<u8>> {
        self.whitespace();
        if self.input.get(self.pos) != Some(&b'"') {
            return Err(invalid_entities());
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos] != b'"' {
            if self.input[self.pos] == 0 || self.pos - start >= MAX_TOKEN {
                return Err(invalid_entities());
            }
            self.pos += 1;
        }
        if self.pos == self.input.len() {
            return Err(invalid_entities());
        }
        let value = self.input[start..self.pos].to_vec();
        self.pos += 1;
        Ok(value)
    }
}

/// Parse the bounded entity text, without executing entity callbacks or spawning a world.
pub fn parse_entities(data: &[u8]) -> io::Result<Vec<MapEntity>> {
    if data.is_empty() || data.len() as u64 > MAX_ENTITY_BYTES {
        return Err(invalid_entities());
    }
    let text = data.strip_suffix(&[0]).unwrap_or(data);
    if text.contains(&0) {
        return Err(invalid_entities());
    }
    let mut parser = EntityParser {
        input: text,
        pos: 0,
    };
    let mut entities = Vec::new();
    loop {
        parser.whitespace();
        if parser.pos == text.len() {
            break;
        }
        if text[parser.pos] != b'{' || entities.len() == MAX_ENTITIES {
            return Err(invalid_entities());
        }
        parser.pos += 1;
        let mut fields = Vec::new();
        loop {
            parser.whitespace();
            if text.get(parser.pos) == Some(&b'}') {
                parser.pos += 1;
                break;
            }
            if fields.len() == MAX_FIELDS {
                return Err(invalid_entities());
            }
            let key = parser.quoted()?;
            if key.is_empty() {
                return Err(invalid_entities());
            }
            let value = parser.quoted()?;
            fields.push((key, value));
        }
        entities.push(MapEntity { fields });
    }
    let Some(world) = entities.first() else {
        return Err(invalid_entities());
    };
    let classnames: Vec<_> = world
        .fields
        .iter()
        .filter(|(key, _)| key == b"classname")
        .collect();
    if classnames.is_empty() || classnames.iter().any(|(_, value)| value != b"worldspawn") {
        return Err(invalid_entities());
    }
    Ok(entities)
}

fn read_entities(reader: &mut (impl Read + Seek), size: u64) -> io::Result<(Vec<MapEntity>, u32)> {
    let (lumps, crc) = map_crc(reader, size)?;
    let lump = &lumps[0];
    if lump.length == 0 || lump.length > MAX_ENTITY_BYTES {
        return Err(invalid_entities());
    }
    reader.seek(SeekFrom::Start(lump.offset))?;
    let mut bytes = vec![0; lump.length as usize];
    reader.read_exact(&mut bytes)?;
    Ok((parse_entities(&bytes)?, crc))
}

/// Read map entities and the multiplayer CRC from the same open BSP file.
pub fn load_entities(path: &Path) -> io::Result<(Vec<MapEntity>, u32)> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "map is not a regular file",
        ));
    }
    read_entities(&mut file, metadata.len())
}

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
    fn parses_worldspawn_and_entities_as_bytes() {
        let input = b"// header\n{\n\"classname\" \"worldspawn\"\n\"message\" \"hi\xff\"\n}\n{\"classname\" \"info_player_start\"}\n\0";
        let entities = parse_entities(input).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].fields[1].1, b"hi\xff");
        assert_eq!(entities[1].fields[0].1, b"info_player_start");
        assert!(
            parse_entities(b"{\"classname\" \"worldspawn\" \"classname\" \"worldspawn\"}").is_ok()
        );
    }

    #[test]
    fn reads_valid_bsp_and_rejects_truncated_entity_lump() {
        let entity = b"{\"classname\" \"worldspawn\"}\0";
        let mut bsp = vec![0; HEADER_SIZE];
        bsp[..4].copy_from_slice(&30_i32.to_le_bytes());
        bsp[4..8].copy_from_slice(&(HEADER_SIZE as i32).to_le_bytes());
        bsp[8..12].copy_from_slice(&(entity.len() as i32).to_le_bytes());
        bsp.extend_from_slice(entity);
        let len = bsp.len() as u64;
        let (entities, _) = read_entities(&mut std::io::Cursor::new(&bsp), len).unwrap();
        assert_eq!(entities.len(), 1);
        assert!(read_entities(&mut std::io::Cursor::new(&bsp[..bsp.len() - 1]), len).is_err());
        bsp[8..12].copy_from_slice(&((entity.len() + 1) as i32).to_le_bytes());
        assert!(read_entities(&mut std::io::Cursor::new(&bsp), len).is_err());
    }

    #[test]
    fn rejects_incomplete_or_ambiguous_entity_lumps() {
        for input in [
            b"".as_slice(),
            b"{}".as_slice(),
            b"{\"classname\" \"other\"}".as_slice(),
            b"{\"classname\" \"worldspawn\" \"classname\" \"other\"}".as_slice(),
            b"{\"classname\" \"worldspawn\"".as_slice(),
            b"{\"classname\"}".as_slice(),
            b"{\"\" \"worldspawn\"}".as_slice(),
            b"{\"classname\" \"worldspawn\"}\0junk".as_slice(),
            b"{\"classname\" \"worldspawn\"} garbage".as_slice(),
        ] {
            assert!(
                parse_entities(input).is_err(),
                "accepted invalid entity data"
            );
        }
        let too_long = format!(
            "{{\"classname\" \"worldspawn\" \"k\" \"{}\"}}",
            "x".repeat(MAX_TOKEN + 1)
        );
        assert!(parse_entities(too_long.as_bytes()).is_err());
        let fields = format!(
            "{{\"classname\" \"worldspawn\" {}}}",
            "\"k\" \"v\" ".repeat(MAX_FIELDS)
        );
        assert!(parse_entities(fields.as_bytes()).is_err());
        assert!(parse_entities(&vec![b' '; MAX_ENTITY_BYTES as usize + 1]).is_err());
    }

    #[test]
    #[ignore = "requires local HLDS map"]
    fn loads_entities_from_local_reference_map() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let (entities, crc) = load_entities(&path).unwrap();
        assert!(entities.len() > 1);
        assert_eq!(crc, 0x451b6da9);
        assert!(
            entities[0]
                .fields
                .iter()
                .any(|(key, value)| key == b"classname" && value == b"worldspawn")
        );
    }

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
