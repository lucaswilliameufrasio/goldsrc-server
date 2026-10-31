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

/// Encode an uncompressed list with no URL, custom data, reserved data or consistency checks.
pub fn encode(spawn_count: u32, resources: &[Resource<'_>]) -> io::Result<Vec<u8>> {
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
        bits.write(0, 1)?; // no reserved data
    }
    bits.write(0, 1)?; // no consistency list
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_and_out_of_range_fields() {
        let base = Resource {
            kind: 0,
            filename: "sound/test.wav",
            index: 1,
            download_size: 10,
            flags: 0,
        };
        assert!(encode(1, &[Resource { flags: 4, ..base }]).is_err());
        let base = Resource {
            kind: 0,
            filename: "sound/test.wav",
            index: 1,
            download_size: 10,
            flags: 0,
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
            },
            Resource {
                kind: 2,
                filename: "models/test.mdl",
                index: 12,
                download_size: 4567,
                flags: 1,
            },
        ];
        assert_eq!(encode(1, &resources).unwrap(), expected);
    }
}
