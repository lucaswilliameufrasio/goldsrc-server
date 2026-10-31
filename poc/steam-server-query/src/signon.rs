//! Experimental sign-on message preparation. Compiled only in tests.
use crate::serverinfo::ServerInfo;
use std::io::{self, Error, ErrorKind};

const MAX_MESSAGE: usize = 65_536;

/// Values in HLDS wire order, either side of the footsteps byte.
pub struct Movevars<'a> {
    pub before_footsteps: [f32; 16],
    pub footsteps: bool,
    pub after_footsteps: [f32; 8],
    pub sky_name: &'a str,
}

impl Movevars<'_> {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.sky_name.len() > 31 || self.sky_name.as_bytes().contains(&0) {
            return Err(Error::new(ErrorKind::InvalidInput, "invalid sky name"));
        }
        let mut out = vec![44]; // svc_newmovevars
        for value in self.before_footsteps {
            if !value.is_finite() {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "invalid movevars float",
                ));
            }
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.push(u8::from(self.footsteps));
        for value in self.after_footsteps {
            if !value.is_finite() {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "invalid movevars float",
                ));
            }
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(self.sky_name.as_bytes());
        out.push(0);
        Ok(out)
    }
}

pub struct Signon<'a> {
    pub info: ServerInfo<'a>,
    pub banner: Option<&'a str>,
    pub fallback: &'a str,
    pub cheats: bool,
    pub delta_descriptions: &'a [u8],
    pub movevars: Movevars<'a>,
    pub cd_track: u8,
}

impl Signon<'_> {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self
            .banner
            .is_some_and(|s| s.len() > 2047 || s.as_bytes().contains(&0))
            || self.fallback.len() > 255
            || self.fallback.as_bytes().contains(&0)
            || self.delta_descriptions.first() != Some(&14)
        {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "invalid sign-on fields",
            ));
        }
        let info = self.info.encode()?;
        let movevars = self.movevars.encode()?;
        let total = info.len()
            + self.banner.map_or(0, |s| s.len() + 2)
            + self.fallback.len()
            + 3
            + self.delta_descriptions.len()
            + movevars.len()
            + 6;
        if total > MAX_MESSAGE {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "sign-on message too large",
            ));
        }
        let mut out = Vec::with_capacity(total);
        if let Some(banner) = self.banner {
            out.push(8); // svc_print
            out.extend_from_slice(banner.as_bytes());
            out.push(0);
        }
        out.extend_from_slice(&info);
        out.push(54); // svc_sendextrainfo
        out.extend_from_slice(self.fallback.as_bytes());
        out.push(0);
        out.push(u8::from(self.cheats));
        out.extend_from_slice(self.delta_descriptions);
        out.extend_from_slice(&movevars);
        out.extend_from_slice(&[32, self.cd_track, self.cd_track, 5]); // cdtrack, setview
        out.extend_from_slice(&(u16::from(self.info.player_index) + 1).to_le_bytes());
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn synthetic<'a>(delta: &'a [u8]) -> Signon<'a> {
        Signon {
            info: ServerInfo {
                spawn_count: 1,
                world_crc: 0x451b6da9,
                client_dll_md5: [0; 16],
                max_players: 2,
                player_index: 0,
                game_type: 1,
                game_dir: "cstrike",
                hostname: "test",
                world_model: "maps/test.bsp",
                map_cycle: "",
                secure: false,
            },
            banner: None,
            fallback: "",
            cheats: false,
            delta_descriptions: delta,
            movevars: Movevars {
                before_footsteps: [1.0; 16],
                footsteps: true,
                after_footsteps: [2.0; 8],
                sky_name: "test",
            },
            cd_track: 0,
        }
    }

    #[test]
    fn rejects_invalid_inputs_and_oversize() {
        assert!(synthetic(&[]).encode().is_err());
        let mut signon = synthetic(&[14]);
        signon.movevars.before_footsteps[0] = f32::NAN;
        assert!(signon.encode().is_err());
        signon.movevars.before_footsteps[0] = 1.0;
        signon.fallback = "bad\0string";
        assert!(signon.encode().is_err());
        let mut delta = vec![0; MAX_MESSAGE];
        delta[0] = 14;
        assert!(synthetic(&delta).encode().is_err());
    }

    #[test]
    #[ignore = "requires local HLDS captures and delta sources"]
    fn assembled_message_matches_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = std::path::Path::new(&root);
        let reference = std::fs::read(root.join("hlds-serverinfo-build10211.bin")).unwrap();
        let delta = steam_server_query_poc::delta::load(
            &root.join("reference-hlds/cstrike/delta.lst"),
            &root.join("hlds-delta-fields-build10211.tsv"),
        )
        .unwrap();
        let mut signon = synthetic(&delta.bytes);
        let banner = "\x02\nBUILD 10211 SERVER (0 CRC)\nServer # 1\n";
        let mut offset = 1 + banner.len() + 1 + 32;
        fn field(bytes: &[u8], offset: &mut usize) -> String {
            let length = bytes[*offset..].iter().position(|b| *b == 0).unwrap();
            let value = String::from_utf8(bytes[*offset..*offset + length].to_vec()).unwrap();
            *offset += length + 1;
            value
        }
        assert_eq!(field(&reference, &mut offset), "cstrike");
        let hostname = field(&reference, &mut offset);
        let model = field(&reference, &mut offset);
        let mapcycle = field(&reference, &mut offset);
        signon.info.hostname = &hostname;
        signon.info.world_model = &model;
        signon.info.map_cycle = &mapcycle;
        signon.banner = Some(banner);
        signon.movevars = Movevars {
            before_footsteps: [
                800.0, 100.0, 900.0, 500.0, 10.0, 10.0, 10.0, 4.0, 2.0, 1.0, 1.0, 1.0, 18.0,
                2000.0, 8192.0, 0.0,
            ],
            footsteps: true,
            after_footsteps: [0.0, 0.0, 197.0, 197.0, 129.0, 0.365677, 0.340999, -0.866025],
            sky_name: "des",
        };
        assert_eq!(
            signon.movevars.encode().unwrap(),
            std::fs::read(root.join("hlds-movevars-build10211.bin")).unwrap()
        );
        assert_eq!(signon.encode().unwrap(), reference);
    }
}
