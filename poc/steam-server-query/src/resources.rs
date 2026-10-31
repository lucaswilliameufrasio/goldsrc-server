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

#[derive(Debug, PartialEq, Eq)]
pub enum ConsistencyError {
    InvalidLength,
    InvalidManifest,
    Malformed,
    HashMismatch,
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
        checks[index] = Some(item.hash);
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
        let expected_hash = checks
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(ConsistencyError::Malformed)?;
        let actual_hash = reader.read(32)?;
        if actual_hash != expected_hash {
            return Err(ConsistencyError::HashMismatch);
        }
        count += 1;
    }
    if count != expected.len() || reader.bit.div_ceil(8) != decoded.len() {
        return Err(ConsistencyError::Malformed);
    }
    Ok(consumed)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        const TABLE: [u8; 16] = [
            0x7a, 0x64, 0x05, 0xf1, 0x1b, 0x9b, 0xa0, 0xb5, 0xca, 0xed, 0x61, 0x0d, 0x4a, 0xdf,
            0x8e, 0xc7,
        ];
        for (word, bytes) in bits.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let value = (u32::from_le_bytes(*bytes) ^ !1_u32).swap_bytes();
            let mut transformed = value.to_le_bytes();
            for (index, byte) in transformed.iter_mut().enumerate() {
                *byte ^= 0xa5 | ((index as u8) << index) | index as u8 | TABLE[(word + index) & 15];
            }
            *bytes = (u32::from_le_bytes(transformed) ^ 1).to_le_bytes();
        }
        let mut frame = (bits.data.len() as u16).to_le_bytes().to_vec();
        frame.extend_from_slice(&bits.data);
        frame
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
