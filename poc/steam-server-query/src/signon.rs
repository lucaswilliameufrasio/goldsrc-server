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

pub struct UserMessage<'a> {
    pub id: u8,
    /// -1 denotes variable size; other sizes are encoded as one byte.
    pub size: i16,
    pub name: &'a str,
}

pub struct ClientUpdate<'a> {
    pub slot: u8,
    pub userid: u32,
    pub userinfo: &'a str,
    /// Caller-supplied protocol digest; this module never fabricates one.
    pub digest: [u8; 16],
}

pub struct NewResponse<'a> {
    pub serverinfo: Signon<'a>,
    pub user_messages: &'a [UserMessage<'a>],
    pub full_server_info: &'a str,
    pub clients: &'a [ClientUpdate<'a>],
}

/// Validate key/value encoding and remove private keys from player updates.
fn checked_info(input: &str, strip_private: bool) -> io::Result<String> {
    if input.len() > 511
        || input
            .bytes()
            .any(|b| b < 32 || b == 127 || b == b'"' || b == b';')
    {
        return Err(Error::new(ErrorKind::InvalidInput, "invalid info string"));
    }
    if input.is_empty() {
        return Ok(String::new());
    }
    let rest = input
        .strip_prefix('\\')
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid info string"))?;
    let mut parts = rest.split('\\');
    let mut output = String::new();
    while let Some(key) = parts.next() {
        let value = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid info string"))?;
        if key.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput, "invalid info key"));
        }
        if !strip_private || !key.starts_with('_') {
            output.push('\\');
            output.push_str(key);
            output.push('\\');
            output.push_str(value);
        }
    }
    Ok(output)
}

impl NewResponse<'_> {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.user_messages.len() > 192 || self.clients.len() > 32 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "too many sign-on records",
            ));
        }
        let full_info = checked_info(self.full_server_info, false)?;
        let mut out = self.serverinfo.encode()?;
        let mut ids = [false; 256];
        for message in self.user_messages {
            if message.id < 64
                || ids[usize::from(message.id)]
                || !(-1..=255).contains(&message.size)
                || message.name.is_empty()
                || message.name.len() > 15
                || !message.name.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(Error::new(ErrorKind::InvalidInput, "invalid user message"));
            }
            ids[usize::from(message.id)] = true;
            out.extend_from_slice(&[39, message.id, message.size as u8]);
            let mut name = [0_u8; 16];
            name[..message.name.len()].copy_from_slice(message.name.as_bytes());
            out.extend_from_slice(&name);
        }
        out.push(9); // svc_stufftext
        out.extend_from_slice(b"fullserverinfo \"");
        out.extend_from_slice(full_info.as_bytes());
        out.extend_from_slice(b"\"\n\0");
        let mut slots = [false; 32];
        for client in self.clients {
            if client.slot >= self.serverinfo.info.max_players || slots[usize::from(client.slot)] {
                return Err(Error::new(ErrorKind::InvalidInput, "invalid client slot"));
            }
            slots[usize::from(client.slot)] = true;
            let info = checked_info(client.userinfo, true)?;
            out.extend_from_slice(&[13, client.slot]); // svc_updateuserinfo
            out.extend_from_slice(&client.userid.to_le_bytes());
            out.extend_from_slice(info.as_bytes());
            out.push(0);
            out.extend_from_slice(&client.digest);
        }
        if out.len() > MAX_MESSAGE {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "sign-on message too large",
            ));
        }
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
    fn new_response_filters_private_info_and_rejects_unsafe_records() {
        let registrations = [UserMessage {
            id: 64,
            size: -1,
            name: "Probe",
        }];
        let clients = [ClientUpdate {
            slot: 0,
            userid: 42,
            userinfo: "\\name\\tester\\_secret\\fake\\rate\\25000",
            digest: [9; 16],
        }];
        let new = NewResponse {
            serverinfo: synthetic(&[14]),
            user_messages: &registrations,
            full_server_info: "\\hostname\\test",
            clients: &clients,
        };
        let bytes = new.encode().unwrap();
        assert!(!bytes.windows(7).any(|s| s == b"_secret"));
        let public_info = b"\\name\\tester\\rate\\25000";
        assert!(bytes.windows(public_info.len()).any(|s| s == public_info));
        assert!(checked_info("\\name\\x\\orphan", true).is_err());
        assert!(checked_info("\\name\\bad\"\n", true).is_err());
        let duplicate = [
            UserMessage {
                id: 64,
                size: 1,
                name: "A",
            },
            UserMessage {
                id: 64,
                size: 2,
                name: "B",
            },
        ];
        assert!(
            NewResponse {
                serverinfo: synthetic(&[14]),
                user_messages: &duplicate,
                full_server_info: "",
                clients: &[]
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires synthetic output captured from the local HLDS"]
    fn new_response_tail_matches_local_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let expected =
            std::fs::read(std::path::Path::new(&root).join("hlds-new-tail-synthetic-v2.bin"))
                .unwrap();
        let messages = [
            UserMessage {
                id: 64,
                size: -1,
                name: "Probe",
            },
            UserMessage {
                id: 65,
                size: 5,
                name: "Test",
            },
        ];
        let clients = [ClientUpdate {
            slot: 0,
            userid: 42,
            userinfo: "\\name\\tester\\_secret\\fake\\rate\\25000",
            digest: [
                0x3b, 0x5d, 0x3c, 0x7d, 0x20, 0x7e, 0x37, 0xdc, 0xee, 0xed, 0xd3, 0x01, 0xe3, 0x5e,
                0x2e, 0x58,
            ],
        }];
        let base = synthetic(&[14]).encode().unwrap();
        let actual = NewResponse {
            serverinfo: synthetic(&[14]),
            user_messages: &messages,
            full_server_info: "\\hostname\\test",
            clients: &clients,
        }
        .encode()
        .unwrap();
        assert_eq!(&actual[..base.len()], &base);
        assert_eq!(&actual[base.len()..], &expected);
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
