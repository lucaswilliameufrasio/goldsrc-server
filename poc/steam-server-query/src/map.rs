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

/// A BSP plane, in on-disk coordinates.
#[derive(Debug, PartialEq)]
pub struct Plane {
    pub normal: [f32; 3],
    pub distance: f32,
    pub kind: i32,
}

/// A BSP model and its root hull references; not a live game entity.
#[derive(Debug, PartialEq)]
pub struct Model {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub origin: [f32; 3],
    pub headnodes: [i32; 4],
    pub visleafs: i32,
    pub first_face: u32,
    pub face_count: u32,
}

#[derive(Debug, PartialEq)]
pub struct Geometry {
    pub planes: Vec<Plane>,
    pub models: Vec<Model>,
    pub nodes: Vec<Node>,
    pub leaves: Vec<Leaf>,
    pub clipnodes: Vec<Clipnode>,
    pub node_count: usize,
    pub clipnode_count: usize,
    pub face_count: usize,
}

#[derive(Debug, PartialEq)]
pub struct Node {
    pub plane: u32,
    /// Nonnegative indices refer to nodes; negative indices encode `-1 - leaf`.
    pub children: [i16; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_face: u16,
    pub face_count: u16,
}

#[derive(Debug, PartialEq)]
pub struct Leaf {
    pub contents: i32,
    pub visibility_offset: i32,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_marksurface: u16,
    pub marksurface_count: u16,
    pub ambient: [u8; 4],
}

#[derive(Debug, PartialEq)]
pub struct Clipnode {
    pub plane: u32,
    /// Nonnegative indices refer to clipnodes; negative values denote contents.
    pub children: [i16; 2],
}

fn read_i16(record: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(record[offset..offset + 2].try_into().unwrap())
}

fn read_u16(record: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(record[offset..offset + 2].try_into().unwrap())
}

fn bounds(record: &[u8], offset: usize) -> ([i16; 3], [i16; 3]) {
    (
        std::array::from_fn(|i| read_i16(record, offset + 2 * i)),
        std::array::from_fn(|i| read_i16(record, offset + 6 + 2 * i)),
    )
}

/// Visit every component iteratively; shared descendants are allowed, cycles are not.
fn reject_cycles(children: &[[i16; 2]]) -> io::Result<()> {
    let mut state = vec![0_u8; children.len()];
    let mut stack = Vec::new();
    for root in 0..children.len() {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        stack.push((root, 0_usize));
        while let Some((node, edge)) = stack.last_mut() {
            if *edge == 2 {
                state[*node] = 2;
                stack.pop();
                continue;
            }
            let child = children[*node][*edge];
            *edge += 1;
            if child < 0 {
                continue;
            }
            let index = child as usize;
            if index >= children.len() || state[index] == 1 {
                return Err(invalid_geometry());
            }
            if state[index] == 0 {
                state[index] = 1;
                stack.push((index, 0));
            }
        }
    }
    Ok(())
}

fn invalid_geometry() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid BSP geometry")
}

fn read_i32(record: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(record[offset..offset + 4].try_into().unwrap())
}

fn read_vec3(record: &[u8], offset: usize) -> [f32; 3] {
    std::array::from_fn(|index| f32::from_bits(read_i32(record, offset + 4 * index) as u32))
}

fn records(lump: &Lump, width: usize) -> io::Result<usize> {
    if lump.length > 16 * 1024 * 1024 || !lump.length.is_multiple_of(width as u64) {
        return Err(invalid_geometry());
    }
    Ok((lump.length / width as u64) as usize)
}

fn geometry_from_reader(reader: &mut (impl Read + Seek), size: u64) -> io::Result<Geometry> {
    let mut header = [0; HEADER_SIZE];
    reader.seek(SeekFrom::Start(0))?;
    reader.read_exact(&mut header)?;
    let lumps = parse_header(&header, size)?;
    let plane_count = records(&lumps[1], 20)?;
    let node_count = records(&lumps[5], 24)?;
    let face_count = records(&lumps[7], 20)?;
    let clipnode_count = records(&lumps[9], 8)?;
    let leaf_count = records(&lumps[10], 28)?;
    let marksurface_count = records(&lumps[11], 2)?;
    let model_count = records(&lumps[14], 64)?;
    if plane_count == 0 || model_count == 0 || node_count == 0 || leaf_count == 0 {
        return Err(invalid_geometry());
    }
    let mut planes = Vec::with_capacity(plane_count);
    reader.seek(SeekFrom::Start(lumps[1].offset))?;
    for _ in 0..plane_count {
        let mut record = [0; 20];
        reader.read_exact(&mut record)?;
        let normal = read_vec3(&record, 0);
        let distance = f32::from_bits(read_i32(&record, 12) as u32);
        let kind = read_i32(&record, 16);
        if !normal.iter().all(|value| value.is_finite())
            || !distance.is_finite()
            || normal == [0.0; 3]
            || !(0..=5).contains(&kind)
        {
            return Err(invalid_geometry());
        }
        planes.push(Plane {
            normal,
            distance,
            kind,
        });
    }
    let mut nodes = Vec::with_capacity(node_count);
    reader.seek(SeekFrom::Start(lumps[5].offset))?;
    for _ in 0..node_count {
        let mut record = [0; 24];
        reader.read_exact(&mut record)?;
        let plane = read_i32(&record, 0);
        let children = [read_i16(&record, 4), read_i16(&record, 6)];
        let (mins, maxs) = bounds(&record, 8);
        let first_face = read_u16(&record, 20);
        let count = read_u16(&record, 22);
        if plane < 0
            || plane as usize >= plane_count
            || (0..3).any(|i| mins[i] > maxs[i])
            || usize::from(first_face) + usize::from(count) > face_count
            || children.iter().any(|child| {
                if *child >= 0 {
                    *child as usize >= node_count
                } else {
                    (-1_i32 - i32::from(*child)) as usize >= leaf_count
                }
            })
        {
            return Err(invalid_geometry());
        }
        nodes.push(Node {
            plane: plane as u32,
            children,
            mins,
            maxs,
            first_face,
            face_count: count,
        });
    }
    let mut leaves = Vec::with_capacity(leaf_count);
    reader.seek(SeekFrom::Start(lumps[10].offset))?;
    for _ in 0..leaf_count {
        let mut record = [0; 28];
        reader.read_exact(&mut record)?;
        let contents = read_i32(&record, 0);
        let visibility_offset = read_i32(&record, 4);
        let (mins, maxs) = bounds(&record, 8);
        let first_marksurface = read_u16(&record, 20);
        let count = read_u16(&record, 22);
        if contents >= 0
            || visibility_offset < -1
            || (visibility_offset >= 0 && visibility_offset as u64 >= lumps[4].length)
            || (0..3).any(|i| mins[i] > maxs[i])
            || usize::from(first_marksurface) + usize::from(count) > marksurface_count
        {
            return Err(invalid_geometry());
        }
        leaves.push(Leaf {
            contents,
            visibility_offset,
            mins,
            maxs,
            first_marksurface,
            marksurface_count: count,
            ambient: record[24..28].try_into().unwrap(),
        });
    }
    let mut clipnodes = Vec::with_capacity(clipnode_count);
    reader.seek(SeekFrom::Start(lumps[9].offset))?;
    for _ in 0..clipnode_count {
        let mut record = [0; 8];
        reader.read_exact(&mut record)?;
        let plane = read_i32(&record, 0);
        let children = [read_i16(&record, 4), read_i16(&record, 6)];
        if plane < 0
            || plane as usize >= plane_count
            || children
                .iter()
                .any(|child| *child >= 0 && *child as usize >= clipnode_count)
        {
            return Err(invalid_geometry());
        }
        clipnodes.push(Clipnode {
            plane: plane as u32,
            children,
        });
    }
    reject_cycles(&nodes.iter().map(|node| node.children).collect::<Vec<_>>())?;
    reject_cycles(
        &clipnodes
            .iter()
            .map(|node| node.children)
            .collect::<Vec<_>>(),
    )?;
    let mut models = Vec::with_capacity(model_count);
    reader.seek(SeekFrom::Start(lumps[14].offset))?;
    for _ in 0..model_count {
        let mut record = [0; 64];
        reader.read_exact(&mut record)?;
        let mins = read_vec3(&record, 0);
        let maxs = read_vec3(&record, 12);
        let origin = read_vec3(&record, 24);
        let headnodes: [i32; 4] = std::array::from_fn(|index| read_i32(&record, 36 + index * 4));
        let visleafs = read_i32(&record, 52);
        let first = read_i32(&record, 56);
        let count = read_i32(&record, 60);
        if !mins
            .iter()
            .chain(maxs.iter())
            .chain(origin.iter())
            .all(|value| value.is_finite())
            || (0..3).any(|index| mins[index] > maxs[index])
            || headnodes[0] < 0
            || headnodes[0] as usize >= node_count
            || headnodes[1..]
                .iter()
                .any(|index| *index >= 0 && *index as usize >= clipnode_count)
            || visleafs < 0
            || visleafs as usize > leaf_count
            || first < 0
            || count < 0
            || (first as usize)
                .checked_add(count as usize)
                .is_none_or(|end| end > face_count)
        {
            return Err(invalid_geometry());
        }
        models.push(Model {
            mins,
            maxs,
            origin,
            headnodes,
            visleafs,
            first_face: first as u32,
            face_count: count as u32,
        });
    }
    Ok(Geometry {
        planes,
        models,
        nodes,
        leaves,
        clipnodes,
        node_count,
        clipnode_count,
        face_count,
    })
}

/// Load a bounded geometry subset; face and marksurface entries are not decoded yet.
pub fn load_geometry(path: &Path) -> io::Result<Geometry> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "map is not a regular file",
        ));
    }
    geometry_from_reader(&mut file, metadata.len())
}

/// Load the available world subset without instantiating gameplay state.
pub fn load_world(path: &Path) -> io::Result<(Vec<MapEntity>, u32, Geometry)> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "map is not a regular file",
        ));
    }
    let (entities, crc) = read_entities(&mut file, metadata.len())?;
    let geometry = geometry_from_reader(&mut file, metadata.len())?;
    Ok((entities, crc, geometry))
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

    fn synthetic_geometry() -> Vec<u8> {
        let mut bsp = vec![0; HEADER_SIZE];
        bsp[..4].copy_from_slice(&30_i32.to_le_bytes());
        let mut plane = vec![0_u8; 20];
        plane[..4].copy_from_slice(&1.0_f32.to_le_bytes());
        let mut model = vec![0_u8; 64];
        let mut node = vec![0_u8; 24];
        node[4..8].copy_from_slice(&[255; 4]);
        let mut leaf = vec![0_u8; 28];
        leaf[..8].copy_from_slice(&[255; 8]);
        let mut clipnode = vec![0_u8; 8];
        clipnode[4..8].copy_from_slice(&[255; 4]);
        for index in 1..4 {
            model[36 + index * 4..40 + index * 4].copy_from_slice(&(-1_i32).to_le_bytes());
        }
        for (index, bytes) in [
            (1, plane),
            (5, node),
            (9, clipnode),
            (10, leaf),
            (14, model),
        ] {
            let position = 4 + index * 8;
            let offset = bsp.len() as i32;
            bsp[position..position + 4].copy_from_slice(&offset.to_le_bytes());
            bsp[position + 4..position + 8].copy_from_slice(&(bytes.len() as i32).to_le_bytes());
            bsp.extend_from_slice(&bytes);
        }
        bsp
    }

    #[test]
    fn geometry_loads_and_rejects_invalid_model_references() {
        let original = synthetic_geometry();
        let geometry =
            geometry_from_reader(&mut std::io::Cursor::new(&original), original.len() as u64)
                .unwrap();
        assert_eq!(geometry.planes.len(), 1);
        assert_eq!(geometry.models.len(), 1);
        assert_eq!(geometry.planes[0].normal, [1.0, 0.0, 0.0]);
        let model_offset = i32::from_le_bytes(original[116..120].try_into().unwrap()) as usize;
        for (offset, value) in [(36, 1_i32), (40, 1), (52, 2), (56, -1), (60, 1)] {
            let mut invalid = original.clone();
            invalid[model_offset + offset..model_offset + offset + 4]
                .copy_from_slice(&value.to_le_bytes());
            assert!(
                geometry_from_reader(&mut std::io::Cursor::new(&invalid), invalid.len() as u64)
                    .is_err()
            );
        }
        let mut invalid = original.clone();
        invalid[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(
            geometry_from_reader(&mut std::io::Cursor::new(&invalid), invalid.len() as u64)
                .is_err()
        );
        assert!(
            geometry_from_reader(
                &mut std::io::Cursor::new(&original[..original.len() - 1]),
                original.len() as u64
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_malformed_tree_references_and_cycles() {
        let original = synthetic_geometry();
        let lump_offset = |index: usize| -> usize {
            let offset = 4 + index * 8;
            i32::from_le_bytes(original[offset..offset + 4].try_into().unwrap()) as usize
        };
        let node = lump_offset(5);
        let clip = lump_offset(9);
        let leaf = lump_offset(10);
        for (offset, value) in [
            (node, 1_i32.to_le_bytes().to_vec()),     // plane out of bounds
            (node + 4, 1_i16.to_le_bytes().to_vec()), // node out of bounds
            (node + 4, (-2_i16).to_le_bytes().to_vec()), // leaf out of bounds
            (node + 4, 0_i16.to_le_bytes().to_vec()), // node cycle
            (node + 20, 1_u16.to_le_bytes().to_vec()), // face range
            (clip, 1_i32.to_le_bytes().to_vec()),     // plane out of bounds
            (clip + 4, 1_i16.to_le_bytes().to_vec()), // clipnode out of bounds
            (clip + 4, 0_i16.to_le_bytes().to_vec()), // clipnode cycle
            (leaf, 0_i32.to_le_bytes().to_vec()),     // invalid contents
            (leaf + 4, 0_i32.to_le_bytes().to_vec()), // no visibility data
            (leaf + 20, 1_u16.to_le_bytes().to_vec()), // marksurface range
        ] {
            let mut invalid = original.clone();
            invalid[offset..offset + value.len()].copy_from_slice(&value);
            assert!(
                geometry_from_reader(&mut std::io::Cursor::new(&invalid), invalid.len() as u64)
                    .is_err()
            );
        }
        assert!(reject_cycles(&[[1, -1], [0, -1]]).is_err());
        assert!(reject_cycles(&[[-1, -1], [2, -1], [1, -1]]).is_err());
        assert!(reject_cycles(&[[1, 2], [2, -1], [-1, -1]]).is_ok());
        let mut chain = Vec::new();
        for index in 0..30_000_i16 {
            chain.push([if index == 29_999 { -1 } else { index + 1 }, -1]);
        }
        assert!(reject_cycles(&chain).is_ok());
    }

    #[test]
    #[ignore = "requires local HLDS map"]
    fn loads_geometry_from_local_reference_map() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let geometry = load_geometry(&path).unwrap();
        assert!(!geometry.models.is_empty());
        assert!(!geometry.planes.is_empty());
        assert!(geometry.node_count > 0);
    }

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
