//! Experimental resource-list wire encoding, not connected to live sign-on.
use std::io::{self, Error, ErrorKind};

const MAX_MESSAGE: usize = 65_536;

pub struct Resource<'a> {
    pub kind: u8,
    pub filename: &'a str,
    pub index: u16,
    pub download_size: u32,
    /// Only RES_FATALIFMISSING and RES_WASMISSING are supported.
    pub flags: u8,
    pub reserved: [u8; 32],
}

struct Bits {
    data: Vec<u8>,
    bit: usize,
}

impl Bits {
    fn write(&mut self, value: u32, count: usize) -> io::Result<()> {
        for index in 0..count {
            if self.bit / 8 >= MAX_MESSAGE {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "resource list too large",
                ));
            }
            if self.bit.is_multiple_of(8) {
                self.data.push(0);
            }
            self.data[self.bit / 8] |= (((value >> index) & 1) as u8) << (self.bit % 8);
            self.bit += 1;
        }
        Ok(())
    }
}

/// Encode an uncompressed list with no URL, custom data or consistency checks.
pub fn encode(spawn_count: u32, resources: &[Resource<'_>]) -> io::Result<Vec<u8>> {
    encode_with_consistency(spawn_count, resources, None)
}

/// When present, consistency indices are positions in the manifest, not resource IDs.
pub fn encode_with_consistency(
    spawn_count: u32,
    resources: &[Resource<'_>],
    checks: Option<&[usize]>,
) -> io::Result<Vec<u8>> {
    if resources.len() > 4095 {
        return Err(Error::new(ErrorKind::InvalidInput, "too many resources"));
    }
    let mut bits = Bits {
        data: Vec::new(),
        bit: 0,
    };
    bits.write(resources.len() as u32, 12)?;
    for resource in resources {
        if resource.kind > 15
            || resource.index > 4095
            || resource.download_size > 0x00ff_ffff
            || resource.flags & !3 != 0
            || resource.filename.is_empty()
            || resource.filename.len() > 63
            || !resource.filename.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::new(ErrorKind::InvalidInput, "invalid resource"));
        }
        bits.write(u32::from(resource.kind), 4)?;
        for b in resource.filename.bytes().chain(std::iter::once(0)) {
            bits.write(u32::from(b), 8)?;
        }
        bits.write(u32::from(resource.index), 12)?;
        bits.write(resource.download_size, 24)?;
        bits.write(u32::from(resource.flags), 3)?;
        let has_reserved = resource.reserved.iter().any(|byte| *byte != 0);
        bits.write(u32::from(has_reserved), 1)?;
        if has_reserved {
            for byte in resource.reserved {
                bits.write(u32::from(byte), 8)?;
            }
        }
    }
    if let Some(checks) = checks {
        bits.write(1, 1)?;
        let mut last = 0_usize;
        for (position, index) in checks.iter().copied().enumerate() {
            if index >= resources.len() || (position > 0 && index <= last) {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "invalid consistency index",
                ));
            }
            let difference = index - last;
            bits.write(1, 1)?;
            if difference <= 31 {
                bits.write(1, 1)?;
                bits.write(difference as u32, 5)?;
            } else {
                if index > 1023 {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "consistency index cannot be represented",
                    ));
                }
                bits.write(0, 1)?;
                bits.write(index as u32, 10)?;
            }
            last = index;
        }
        bits.write(0, 1)?;
    } else {
        bits.write(0, 1)?;
    }
    if bits.data.len() + 10 > MAX_MESSAGE {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "resource list too large",
        ));
    }
    let mut out = vec![45]; // svc_resourcerequest
    out.extend_from_slice(&spawn_count.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.push(43); // svc_resourcelist
    out.extend_from_slice(&bits.data);
    Ok(out)
}

/// A hash-based check only. Model-bound checks require separate handling.
pub struct ExpectedHash {
    pub resource_position: u16,
    pub hash: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExpectedValue {
    Hash(u32),
    SameBounds { mins: [f32; 3], maxs: [f32; 3] },
    WithinBounds { mins: [f32; 3], maxs: [f32; 3] },
    WithinBoundsIfAvailable { mins: [f32; 3], maxs: [f32; 3] },
}

#[derive(Debug, PartialEq)]
pub struct ExpectedCheck {
    pub resource_position: u16,
    pub value: ExpectedValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsistencyError {
    InvalidLength,
    InvalidManifest,
    Malformed,
    HashMismatch,
    BoundsMismatch(u16),
}

fn unmunge_response(data: &mut [u8], spawn_count: u32) {
    const TABLE: [u8; 16] = [
        0x7a, 0x64, 0x05, 0xf1, 0x1b, 0x9b, 0xa0, 0xb5, 0xca, 0xed, 0x61, 0x0d, 0x4a, 0xdf, 0x8e,
        0xc7,
    ];
    for (word, bytes) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let mut value = u32::from_le_bytes(*bytes) ^ spawn_count;
        let mut transformed = value.to_le_bytes();
        for (index, byte) in transformed.iter_mut().enumerate() {
            *byte ^= 0xa5 | ((index as u8) << index) | index as u8 | TABLE[(word + index) & 15];
        }
        value = u32::from_le_bytes(transformed).swap_bytes() ^ !spawn_count;
        *bytes = value.to_le_bytes();
    }
}

/// Decode a previously selected RES_CHECKFILE entry using the originating spawn count.
/// The caller must supply the first four bytes of the actual server-side file digest.
pub fn check_from_metadata(
    resource_position: u16,
    hash: [u8; 4],
    reserved: [u8; 32],
    spawn_count: u32,
) -> Result<ExpectedCheck, ConsistencyError> {
    if resource_position >= 4096 {
        return Err(ConsistencyError::InvalidManifest);
    }
    let value = if reserved.iter().all(|byte| *byte == 0) {
        ExpectedValue::Hash(u32::from_le_bytes(hash))
    } else {
        let mut decoded = reserved;
        unmunge_response(&mut decoded, spawn_count);
        let mut coords = [0_f32; 6];
        for (index, item) in coords.iter_mut().enumerate() {
            let start = 1 + index * 4;
            let bytes: [u8; 4] = decoded[start..start + 4]
                .try_into()
                .map_err(|_| ConsistencyError::InvalidManifest)?;
            *item = f32::from_le_bytes(bytes);
        }
        let mins = [coords[0], coords[1], coords[2]];
        let maxs = [coords[3], coords[4], coords[5]];
        for axis in 0..3 {
            if !mins[axis].is_finite() || !maxs[axis].is_finite() || mins[axis] > maxs[axis] {
                return Err(ConsistencyError::InvalidManifest);
            }
        }
        match decoded[0] {
            1 => ExpectedValue::SameBounds { mins, maxs },
            2 => ExpectedValue::WithinBounds { mins, maxs },
            3 => ExpectedValue::WithinBoundsIfAvailable { mins, maxs },
            _ => return Err(ConsistencyError::InvalidManifest),
        }
    };
    Ok(ExpectedCheck {
        resource_position,
        value,
    })
}

struct Reader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl Reader<'_> {
    fn read(&mut self, count: usize) -> Result<u32, ConsistencyError> {
        if self.bit + count > self.data.len() * 8 {
            return Err(ConsistencyError::Malformed);
        }
        let mut value = 0_u32;
        for index in 0..count {
            value |= u32::from((self.data[self.bit / 8] >> (self.bit % 8)) & 1) << index;
            self.bit += 1;
        }
        Ok(value)
    }
}

/// Parse bytes after clc_fileconsistency, returning consumed bytes on a full hash match.
/// Stricter than HLDS: rejects duplicates, omissions and extra bytes within the frame.
/// It does not authenticate a player or support model bounds / game-DLL callbacks.
pub fn validate_hash_response(
    frame: &[u8],
    spawn_count: u32,
    resource_count: usize,
    expected: &[ExpectedHash],
) -> Result<usize, ConsistencyError> {
    let checks: Vec<_> = expected
        .iter()
        .map(|item| ExpectedCheck {
            resource_position: item.resource_position,
            value: ExpectedValue::Hash(item.hash),
        })
        .collect();
    validate_consistency_response(frame, spawn_count, resource_count, &checks)
}

/// Reject inconsistent files; this does not implement a game-DLL override callback.
pub fn validate_consistency_response(
    frame: &[u8],
    spawn_count: u32,
    resource_count: usize,
    expected: &[ExpectedCheck],
) -> Result<usize, ConsistencyError> {
    if frame.len() < 2 {
        return Err(ConsistencyError::InvalidLength);
    }
    let length = i16::from_le_bytes([frame[0], frame[1]]);
    if length <= 0 || usize::from(length as u16) > frame.len() - 2 {
        return Err(ConsistencyError::InvalidLength);
    }
    if resource_count > 4096 || expected.is_empty() || expected.len() > resource_count {
        return Err(ConsistencyError::InvalidManifest);
    }
    let mut checks = vec![None; resource_count];
    for item in expected {
        let index = usize::from(item.resource_position);
        if index >= resource_count || checks[index].is_some() {
            return Err(ConsistencyError::InvalidManifest);
        }
        match item.value {
            ExpectedValue::Hash(_) => {}
            ExpectedValue::SameBounds { mins, maxs }
            | ExpectedValue::WithinBounds { mins, maxs }
            | ExpectedValue::WithinBoundsIfAvailable { mins, maxs } => {
                for axis in 0..3 {
                    if !mins[axis].is_finite() || !maxs[axis].is_finite() || mins[axis] > maxs[axis]
                    {
                        return Err(ConsistencyError::InvalidManifest);
                    }
                }
            }
        }
        checks[index] = Some(item.value);
    }
    let consumed = usize::from(length as u16) + 2;
    let mut decoded = frame[2..consumed].to_vec();
    unmunge_response(&mut decoded, spawn_count);
    let mut reader = Reader {
        data: &decoded,
        bit: 0,
    };
    let mut count = 0_usize;
    while reader.read(1)? != 0 {
        let index = reader.read(12)? as usize;
        let check = checks
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(ConsistencyError::Malformed)?;
        match check {
            ExpectedValue::Hash(expected_hash) => {
                if reader.read(32)? != expected_hash {
                    return Err(ConsistencyError::HashMismatch);
                }
            }
            value => {
                let mut actual = [0_f32; 6];
                for entry in &mut actual {
                    *entry = f32::from_bits(reader.read(32)?);
                    if !entry.is_finite() {
                        return Err(ConsistencyError::Malformed);
                    }
                }
                let (mins, maxs, exact, allow_missing) = match value {
                    ExpectedValue::SameBounds { mins, maxs } => (mins, maxs, true, false),
                    ExpectedValue::WithinBounds { mins, maxs } => (mins, maxs, false, false),
                    ExpectedValue::WithinBoundsIfAvailable { mins, maxs } => {
                        (mins, maxs, false, true)
                    }
                    ExpectedValue::Hash(_) => unreachable!(),
                };
                if !(allow_missing && actual.iter().all(|v| *v == -1.0)) {
                    for axis in 0..3 {
                        let mismatch = if exact {
                            actual[axis] != mins[axis] || actual[axis + 3] != maxs[axis]
                        } else {
                            actual[axis] < mins[axis]
                                || actual[axis + 3] > maxs[axis]
                                || actual[axis] > actual[axis + 3]
                        };
                        if mismatch {
                            return Err(ConsistencyError::BoundsMismatch(index as u16));
                        }
                    }
                }
            }
        }
        count += 1;
    }
    if count != expected.len() || reader.bit.div_ceil(8) != decoded.len() {
        return Err(ConsistencyError::Malformed);
    }
    Ok(consumed)
}

/// Test-only negotiation: passing consistency does not imply resources are available.
#[derive(Debug, PartialEq, Eq)]
enum ResourcePhase {
    AwaitingConsistency,
    AwaitingResources,
    Rejected(ConsistencyError),
}

struct ManifestEntry<'a> {
    resource: Resource<'a>,
    /// Present only for entries selected for consistency; never infer a digest.
    consistency_hash: Option<[u8; 4]>,
}

struct ResourceNegotiation {
    spawn_count: u32,
    resource_count: usize,
    checks: Vec<ExpectedCheck>,
    phase: ResourcePhase,
}

impl ResourceNegotiation {
    fn prepare(spawn_count: u32, entries: &[ManifestEntry<'_>]) -> io::Result<(Self, Vec<u8>)> {
        let mut resources = Vec::with_capacity(entries.len());
        let mut checks = Vec::new();
        let mut positions = Vec::new();
        for (position, entry) in entries.iter().enumerate() {
            if let Some(hash) = entry.consistency_hash {
                let position = u16::try_from(position).map_err(|_| {
                    Error::new(ErrorKind::InvalidInput, "invalid resource position")
                })?;
                let check =
                    check_from_metadata(position, hash, entry.resource.reserved, spawn_count)
                        .map_err(|_| {
                            Error::new(ErrorKind::InvalidInput, "invalid consistency metadata")
                        })?;
                checks.push(check);
                positions.push(usize::from(position));
            }
            resources.push(Resource {
                kind: entry.resource.kind,
                filename: entry.resource.filename,
                index: entry.resource.index,
                download_size: entry.resource.download_size,
                flags: entry.resource.flags,
                reserved: entry.resource.reserved,
            });
        }
        let selected = if checks.is_empty() {
            None
        } else {
            Some(positions.as_slice())
        };
        let message = encode_with_consistency(spawn_count, &resources, selected)?;
        let phase = if checks.is_empty() {
            ResourcePhase::AwaitingResources
        } else {
            ResourcePhase::AwaitingConsistency
        };
        Ok((
            Self {
                spawn_count,
                resource_count: resources.len(),
                checks,
                phase,
            },
            message,
        ))
    }

    fn receive_consistency(&mut self, frame: &[u8]) -> Result<usize, ConsistencyError> {
        if self.phase != ResourcePhase::AwaitingConsistency {
            return Err(ConsistencyError::Malformed);
        }
        match validate_consistency_response(
            frame,
            self.spawn_count,
            self.resource_count,
            &self.checks,
        ) {
            Ok(consumed) => {
                self.phase = ResourcePhase::AwaitingResources;
                Ok(consumed)
            }
            Err(error) => {
                self.phase = ResourcePhase::Rejected(error);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn negotiation_entry(hash: Option<[u8; 4]>) -> ManifestEntry<'static> {
        ManifestEntry {
            resource: Resource {
                kind: 0,
                filename: "test.wav",
                index: 1,
                download_size: 10,
                flags: 0,
                reserved: [0; 32],
            },
            consistency_hash: hash,
        }
    }

    #[test]
    fn negotiation_binds_manifest_checks_and_rejects_replays() {
        let entries = [negotiation_entry(Some(7_u32.to_le_bytes()))];
        let (mut session, message) = ResourceNegotiation::prepare(1, &entries).unwrap();
        assert_eq!(
            message,
            encode_with_consistency(1, &[negotiation_entry(None).resource], Some(&[0])).unwrap()
        );
        assert_eq!(session.phase, ResourcePhase::AwaitingConsistency);
        assert_eq!(
            session.receive_consistency(&synthetic_hash_frame(&[(0, 7)])),
            Ok(8)
        );
        assert_eq!(session.phase, ResourcePhase::AwaitingResources);
        assert_eq!(
            session.receive_consistency(&synthetic_hash_frame(&[(0, 7)])),
            Err(ConsistencyError::Malformed)
        );
        assert_eq!(session.phase, ResourcePhase::AwaitingResources);
        let (mut rejected, _) = ResourceNegotiation::prepare(1, &entries).unwrap();
        assert_eq!(
            rejected.receive_consistency(&synthetic_hash_frame(&[(0, 8)])),
            Err(ConsistencyError::HashMismatch)
        );
        assert_eq!(
            rejected.phase,
            ResourcePhase::Rejected(ConsistencyError::HashMismatch)
        );
        assert_eq!(
            rejected.receive_consistency(&synthetic_hash_frame(&[(0, 7)])),
            Err(ConsistencyError::Malformed)
        );
        assert_eq!(
            rejected.phase,
            ResourcePhase::Rejected(ConsistencyError::HashMismatch)
        );
    }

    #[test]
    fn negotiation_without_checks_still_requires_resources() {
        let (mut session, message) =
            ResourceNegotiation::prepare(1, &[negotiation_entry(None)]).unwrap();
        assert_eq!(
            message,
            encode(1, &[negotiation_entry(None).resource]).unwrap()
        );
        assert_eq!(session.phase, ResourcePhase::AwaitingResources);
        assert_eq!(
            session.receive_consistency(&[]),
            Err(ConsistencyError::Malformed)
        );
        let (mut session, _) =
            ResourceNegotiation::prepare(1, &[negotiation_entry(Some(7_u32.to_le_bytes()))])
                .unwrap();
        assert_eq!(
            session.receive_consistency(&[0, 0]),
            Err(ConsistencyError::InvalidLength)
        );
        assert_eq!(
            session.phase,
            ResourcePhase::Rejected(ConsistencyError::InvalidLength)
        );
    }

    #[test]
    fn hash_metadata_and_invalid_positions_are_checked() {
        assert_eq!(
            check_from_metadata(2, [0x78, 0x56, 0x34, 0x12], [0; 32], 1),
            Ok(ExpectedCheck {
                resource_position: 2,
                value: ExpectedValue::Hash(0x12345678)
            })
        );
        assert_eq!(
            check_from_metadata(4096, [0; 4], [0; 32], 1),
            Err(ConsistencyError::InvalidManifest)
        );
        assert_eq!(
            check_from_metadata(1, [0; 4], [0xff; 32], 1),
            Err(ConsistencyError::InvalidManifest)
        );
    }

    #[test]
    fn bounds_metadata_rejects_unknown_mode_and_invalid_coordinates() {
        let mut raw = [0_u8; 32];
        raw[0] = 2;
        for (index, value) in [-1_f32, -2.0, -3.0, 1.0, 2.0, 3.0].iter().enumerate() {
            raw[1 + index * 4..5 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut encrypted: [u8; 32] = synthetic_frame(raw.to_vec())[2..].try_into().unwrap();
        assert_eq!(
            check_from_metadata(5, [0; 4], encrypted, 1),
            Ok(ExpectedCheck {
                resource_position: 5,
                value: ExpectedValue::WithinBounds {
                    mins: [-1.0, -2.0, -3.0],
                    maxs: [1.0, 2.0, 3.0]
                },
            })
        );
        raw[0] = 9;
        encrypted.copy_from_slice(&synthetic_frame(raw.to_vec())[2..]);
        assert_eq!(
            check_from_metadata(5, [0; 4], encrypted, 1),
            Err(ConsistencyError::InvalidManifest)
        );
        raw[0] = 2;
        raw[1..5].copy_from_slice(&f32::NAN.to_le_bytes());
        encrypted.copy_from_slice(&synthetic_frame(raw.to_vec())[2..]);
        assert_eq!(
            check_from_metadata(5, [0; 4], encrypted, 1),
            Err(ConsistencyError::InvalidManifest)
        );
    }

    #[test]
    #[ignore = "requires local HLDS consistency metadata capture"]
    fn real_consistency_metadata_matches_hlds_decoding() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = std::path::Path::new(&root).join("hlds-consistency-metadata-10211.tsv");
        let input = std::fs::read_to_string(path).unwrap();
        fn hex<const N: usize>(s: &str) -> [u8; N] {
            assert_eq!(s.len(), N * 2);
            let mut result = [0; N];
            for (i, pair) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                result[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            }
            result
        }
        let mut count = 0;
        let mut bounds_count = 0;
        for line in input.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 4);
            let position: u16 = fields[0].parse().unwrap();
            let hash = hex::<4>(fields[1]);
            let raw = hex::<32>(fields[2]);
            let decoded = hex::<32>(fields[3]);
            let actual = check_from_metadata(position, hash, raw, 1).unwrap();
            let expected = if raw == [0; 32] {
                ExpectedValue::Hash(u32::from_le_bytes(hash))
            } else {
                bounds_count += 1;
                let mut values = [0_f32; 6];
                for (index, value) in values.iter_mut().enumerate() {
                    let start = 1 + index * 4;
                    *value = f32::from_le_bytes(decoded[start..start + 4].try_into().unwrap());
                }
                let mins = [values[0], values[1], values[2]];
                let maxs = [values[3], values[4], values[5]];
                match decoded[0] {
                    1 => ExpectedValue::SameBounds { mins, maxs },
                    2 => ExpectedValue::WithinBounds { mins, maxs },
                    3 => ExpectedValue::WithinBoundsIfAvailable { mins, maxs },
                    _ => panic!("HLDS returned an unsupported check type"),
                }
            };
            assert_eq!(
                actual,
                ExpectedCheck {
                    resource_position: position,
                    value: expected
                }
            );
            count += 1;
        }
        assert_eq!((count, bounds_count), (62, 52));
    }

    fn synthetic_hash_frame(entries: &[(u16, u32)]) -> Vec<u8> {
        let mut bits = Bits {
            data: Vec::new(),
            bit: 0,
        };
        for &(index, hash) in entries {
            bits.write(1, 1).unwrap();
            bits.write(u32::from(index), 12).unwrap();
            bits.write(hash, 32).unwrap();
        }
        bits.write(0, 1).unwrap();
        synthetic_frame(bits.data)
    }

    fn synthetic_frame(mut data: Vec<u8>) -> Vec<u8> {
        const TABLE: [u8; 16] = [
            0x7a, 0x64, 0x05, 0xf1, 0x1b, 0x9b, 0xa0, 0xb5, 0xca, 0xed, 0x61, 0x0d, 0x4a, 0xdf,
            0x8e, 0xc7,
        ];
        for (word, bytes) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let value = (u32::from_le_bytes(*bytes) ^ !1_u32).swap_bytes();
            let mut transformed = value.to_le_bytes();
            for (index, byte) in transformed.iter_mut().enumerate() {
                *byte ^= 0xa5 | ((index as u8) << index) | index as u8 | TABLE[(word + index) & 15];
            }
            *bytes = (u32::from_le_bytes(transformed) ^ 1).to_le_bytes();
        }
        let mut frame = (data.len() as u16).to_le_bytes().to_vec();
        frame.extend_from_slice(&data);
        frame
    }

    fn synthetic_bounds_frame(index: u16, values: [f32; 6]) -> Vec<u8> {
        let mut bits = Bits {
            data: Vec::new(),
            bit: 0,
        };
        bits.write(1, 1).unwrap();
        bits.write(u32::from(index), 12).unwrap();
        for value in values {
            bits.write(value.to_bits(), 32).unwrap();
        }
        bits.write(0, 1).unwrap();
        synthetic_frame(bits.data)
    }

    #[test]
    fn model_bounds_reject_inconsistent_nonfinite_and_malformed_responses() {
        let mins = [-1.0, -2.0, -3.0];
        let maxs = [1.0, 2.0, 3.0];
        let values = [-1.0, -2.0, -3.0, 1.0, 2.0, 3.0];
        let same = [ExpectedCheck {
            resource_position: 0,
            value: ExpectedValue::SameBounds { mins, maxs },
        }];
        assert!(
            validate_consistency_response(&synthetic_bounds_frame(0, values), 1, 1, &same).is_ok()
        );
        let mut outside = values;
        outside[0] = -2.0;
        assert_eq!(
            validate_consistency_response(&synthetic_bounds_frame(0, outside), 1, 1, &same),
            Err(ConsistencyError::BoundsMismatch(0))
        );
        let within = [ExpectedCheck {
            resource_position: 0,
            value: ExpectedValue::WithinBounds { mins, maxs },
        }];
        assert_eq!(
            validate_consistency_response(&synthetic_bounds_frame(0, outside), 1, 1, &within),
            Err(ConsistencyError::BoundsMismatch(0))
        );
        let optional = [ExpectedCheck {
            resource_position: 0,
            value: ExpectedValue::WithinBoundsIfAvailable { mins, maxs },
        }];
        assert!(
            validate_consistency_response(&synthetic_bounds_frame(0, [-1.0; 6]), 1, 1, &optional)
                .is_ok()
        );
        assert_eq!(
            validate_consistency_response(&synthetic_bounds_frame(0, [-1.0; 6]), 1, 1, &same),
            Err(ConsistencyError::BoundsMismatch(0))
        );
        let mut nan = values;
        nan[2] = f32::NAN;
        assert_eq!(
            validate_consistency_response(&synthetic_bounds_frame(0, nan), 1, 1, &within),
            Err(ConsistencyError::Malformed)
        );
        let frame = synthetic_bounds_frame(0, values);
        assert_eq!(
            validate_consistency_response(&frame[..frame.len() - 1], 1, 1, &same),
            Err(ConsistencyError::InvalidLength)
        );
    }

    #[test]
    #[ignore = "requires synthetic HLDS model-bounds capture"]
    fn model_bounds_response_matches_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let frame = std::fs::read(
            std::path::Path::new(&root).join("hlds-consistency-bounds-synthetic.bin"),
        )
        .unwrap();
        let mins = [-1.0, -2.0, -3.0];
        let maxs = [1.0, 2.0, 3.0];
        let checks = [
            ExpectedCheck {
                resource_position: 0,
                value: ExpectedValue::SameBounds { mins, maxs },
            },
            ExpectedCheck {
                resource_position: 1,
                value: ExpectedValue::WithinBounds { mins, maxs },
            },
            ExpectedCheck {
                resource_position: 2,
                value: ExpectedValue::WithinBoundsIfAvailable { mins, maxs },
            },
        ];
        assert_eq!(validate_consistency_response(&frame, 1, 3, &checks), Ok(79));
    }

    #[test]
    fn hash_response_rejects_missing_duplicate_and_unrequested_entries() {
        let expected = [
            ExpectedHash {
                resource_position: 0,
                hash: 7,
            },
            ExpectedHash {
                resource_position: 1,
                hash: 8,
            },
        ];
        assert_eq!(
            validate_hash_response(&synthetic_hash_frame(&[(0, 7), (1, 8)]), 1, 3, &expected),
            Ok(14)
        );
        assert_eq!(
            validate_hash_response(&synthetic_hash_frame(&[(0, 7)]), 1, 3, &expected),
            Err(ConsistencyError::Malformed)
        );
        assert_eq!(
            validate_hash_response(&synthetic_hash_frame(&[(0, 7), (0, 7)]), 1, 3, &expected),
            Err(ConsistencyError::Malformed)
        );
        assert_eq!(
            validate_hash_response(&synthetic_hash_frame(&[(0, 7), (2, 8)]), 1, 3, &expected),
            Err(ConsistencyError::Malformed)
        );
        assert_eq!(
            validate_hash_response(&synthetic_hash_frame(&[(0, 7), (3, 8)]), 1, 3, &expected),
            Err(ConsistencyError::Malformed)
        );
    }

    #[test]
    fn validates_hlds_generated_hash_response_and_rejects_invalid_data() {
        // Synthetic capture generated and accepted by official HLDS 10211.
        let frame = [0x06, 0x00, 0x8b, 0xd7, 0x50, 0x00, 0x46, 0x02];
        let expected = [ExpectedHash {
            resource_position: 0,
            hash: 0x12345678,
        }];
        assert_eq!(validate_hash_response(&frame, 1, 1, &expected), Ok(8));
        let mut trailing = frame.to_vec();
        trailing.push(42);
        assert_eq!(validate_hash_response(&trailing, 1, 1, &expected), Ok(8));
        assert_eq!(
            validate_hash_response(&frame[..7], 1, 1, &expected),
            Err(ConsistencyError::InvalidLength)
        );
        assert_eq!(
            validate_hash_response(&[0, 0], 1, 1, &expected),
            Err(ConsistencyError::InvalidLength)
        );
        assert_eq!(
            validate_hash_response(&[0xff, 0xff], 1, 1, &expected),
            Err(ConsistencyError::InvalidLength)
        );
        assert_eq!(
            validate_hash_response(
                &frame,
                1,
                1,
                &[ExpectedHash {
                    resource_position: 0,
                    hash: 0
                }]
            ),
            Err(ConsistencyError::HashMismatch)
        );
        assert_eq!(
            validate_hash_response(
                &frame,
                1,
                1,
                &[ExpectedHash {
                    resource_position: 1,
                    hash: 0
                }]
            ),
            Err(ConsistencyError::InvalidManifest)
        );
        assert_eq!(
            validate_hash_response(
                &frame,
                1,
                2,
                &[
                    ExpectedHash {
                        resource_position: 0,
                        hash: 0x12345678
                    },
                    ExpectedHash {
                        resource_position: 0,
                        hash: 0x12345678
                    }
                ]
            ),
            Err(ConsistencyError::InvalidManifest)
        );
        let mut truncated = frame.to_vec();
        truncated[0] = 5;
        assert_eq!(
            validate_hash_response(&truncated, 1, 1, &expected),
            Err(ConsistencyError::Malformed)
        );
    }

    #[test]
    fn rejects_unsupported_and_out_of_range_fields() {
        let base = Resource {
            kind: 0,
            filename: "sound/test.wav",
            index: 1,
            download_size: 10,
            flags: 0,
            reserved: [0; 32],
        };
        assert!(encode(1, &[Resource { flags: 4, ..base }]).is_err());
        let base = Resource {
            kind: 0,
            filename: "sound/test.wav",
            index: 1,
            download_size: 10,
            flags: 0,
            reserved: [0; 32],
        };
        assert!(
            encode(
                1,
                &[Resource {
                    index: 4096,
                    ..base
                }]
            )
            .is_err()
        );
        let base = Resource {
            kind: 0,
            filename: "sound/test.wav",
            index: 1,
            download_size: 10,
            flags: 0,
            reserved: [0; 32],
        };
        assert!(
            encode(
                1,
                &[Resource {
                    filename: "bad\0path",
                    ..base
                }]
            )
            .is_err()
        );
        assert!(encode(1, &[]).is_ok());
    }

    #[test]
    fn encodes_empty_resource_list_and_no_consistency() {
        assert_eq!(
            encode(7, &[]).unwrap(),
            [45, 7, 0, 0, 0, 0, 0, 0, 0, 43, 0, 0]
        );
    }

    #[test]
    fn nonzero_reserved_data_is_emitted_instead_of_discarded() {
        let plain = Resource {
            kind: 0,
            filename: "test",
            index: 1,
            download_size: 4,
            flags: 0,
            reserved: [0; 32],
        };
        let with_reserved = Resource {
            reserved: [7; 32],
            ..plain
        };
        let plain = Resource {
            kind: 0,
            filename: "test",
            index: 1,
            download_size: 4,
            flags: 0,
            reserved: [0; 32],
        };
        let old = encode(1, &[plain]).unwrap();
        let new = encode(1, &[with_reserved]).unwrap();
        assert_eq!(new.len(), old.len() + 32);
        assert_ne!(new, old);
    }

    #[test]
    #[ignore = "requires synthetic local HLDS resource capture"]
    fn resource_list_matches_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let expected =
            std::fs::read(std::path::Path::new(&root).join("hlds-resources-synthetic.bin"))
                .unwrap();
        let resources = [
            Resource {
                kind: 0,
                filename: "sound/test.wav",
                index: 1,
                download_size: 1234,
                flags: 0,
                reserved: [0; 32],
            },
            Resource {
                kind: 2,
                filename: "models/test.mdl",
                index: 12,
                download_size: 4567,
                flags: 1,
                reserved: [0; 32],
            },
        ];
        assert_eq!(encode(1, &resources).unwrap(), expected);
    }

    #[test]
    #[ignore = "requires local HLDS manifest capture with consistency disabled"]
    fn local_manifest_matches_hlds_without_consistency() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = std::path::Path::new(&root);
        let input = std::fs::read_to_string(root.join("hlds-manifest-10211-v2.tsv")).unwrap();
        let expected = std::fs::read(root.join("hlds-manifest-10211-v2.bin")).unwrap();
        let mut resources = Vec::new();
        let mut reserved_count = 0;
        for line in input.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 6);
            let flags: u8 = fields[4].parse().unwrap();
            // The capture explicitly disables consistency; only its marker may be omitted.
            assert_eq!(flags & !0x83, 0);
            let raw = fields[5].as_bytes();
            assert_eq!(raw.len(), 64);
            let mut reserved = [0_u8; 32];
            for (index, pair) in raw.as_chunks::<2>().0.iter().enumerate() {
                reserved[index] =
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            }
            if reserved.iter().any(|byte| *byte != 0) {
                reserved_count += 1;
            }
            resources.push(Resource {
                filename: fields[0],
                kind: fields[1].parse().unwrap(),
                index: fields[2].parse().unwrap(),
                download_size: fields[3].parse().unwrap(),
                flags: flags & 3,
                reserved,
            });
        }
        assert_eq!(resources.len(), 781);
        assert_eq!(reserved_count, 52);
        assert_eq!(encode(1, &resources).unwrap(), expected);
    }

    #[test]
    fn consistency_checks_reject_duplicates_out_of_range_and_unencodable_gaps() {
        let make = || Resource {
            kind: 0,
            filename: "x",
            index: 1,
            download_size: 1,
            flags: 0,
            reserved: [0; 32],
        };
        let resources: Vec<_> = (0..1100).map(|_| make()).collect();
        assert!(encode_with_consistency(1, &resources, Some(&[0, 31, 63, 1023])).is_ok());
        assert!(encode_with_consistency(1, &resources, Some(&[0, 0])).is_err());
        assert!(encode_with_consistency(1, &resources, Some(&[2, 1])).is_err());
        assert!(encode_with_consistency(1, &resources, Some(&[1100])).is_err());
        assert!(encode_with_consistency(1, &resources, Some(&[1024])).is_err());
        assert!(encode_with_consistency(1, &resources, Some(&[1023, 1054])).is_ok());
    }

    #[test]
    #[ignore = "requires local HLDS manifest and consistency capture"]
    fn local_manifest_matches_hlds_with_consistency() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = std::path::Path::new(&root);
        let input = std::fs::read_to_string(root.join("hlds-manifest-10211-v2.tsv")).unwrap();
        let expected = std::fs::read(root.join("hlds-manifest-consistency-10211.bin")).unwrap();
        let mut resources = Vec::new();
        let mut checks = Vec::new();
        for line in input.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 6);
            let flags: u8 = fields[4].parse().unwrap();
            assert_eq!(flags & !0x83, 0);
            if flags & 0x80 != 0 {
                checks.push(resources.len());
            }
            let raw = fields[5].as_bytes();
            assert_eq!(raw.len(), 64);
            let mut reserved = [0_u8; 32];
            for (index, pair) in raw.as_chunks::<2>().0.iter().enumerate() {
                reserved[index] =
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            }
            resources.push(Resource {
                filename: fields[0],
                kind: fields[1].parse().unwrap(),
                index: fields[2].parse().unwrap(),
                download_size: fields[3].parse().unwrap(),
                flags: flags & 3,
                reserved,
            });
        }
        assert_eq!(resources.len(), 781);
        assert_eq!(checks.len(), 62);
        assert_eq!(expected.len(), 19314);
        assert_eq!(
            encode_with_consistency(1, &resources, Some(&checks)).unwrap(),
            expected
        );
    }
}
