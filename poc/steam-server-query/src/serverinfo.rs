//! Bounded GoldSrc svc_serverinfo encoder. Not wired to live sign-on yet.
use std::io;

/// Explicitly supplied world/client metadata; no implicit map or executable data.
pub struct ServerInfo<'a> {
    pub spawn_count: u32,
    pub encoded_world_crc: u32,
    pub client_dll_md5: [u8; 16],
    pub max_players: u8,
    pub player_index: u8,
    pub game_type: u8,
    pub game_dir: &'a str,
    pub hostname: &'a str,
    pub world_model: &'a str,
    pub map_cycle: &'a str,
    pub secure: bool,
}

impl ServerInfo<'_> {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.max_players == 0
            || self.max_players > 32
            || self.player_index >= self.max_players
            || self.game_type > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid serverinfo player or game type",
            ));
        }
        let fields = [
            self.game_dir,
            self.hostname,
            self.world_model,
            self.map_cycle,
        ];
        for value in fields {
            if value.len() > 255 || value.as_bytes().contains(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid serverinfo string",
                ));
            }
        }
        if self.game_dir.is_empty() || self.world_model.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing serverinfo game directory or world model",
            ));
        }
        let mut message = Vec::with_capacity(64);
        message.push(11); // svc_serverinfo
        message.extend_from_slice(&48_u32.to_le_bytes());
        message.extend_from_slice(&self.spawn_count.to_le_bytes());
        message.extend_from_slice(&self.encoded_world_crc.to_le_bytes());
        message.extend_from_slice(&self.client_dll_md5);
        message.extend_from_slice(&[self.max_players, self.player_index, self.game_type]);
        for value in fields {
            message.extend_from_slice(value.as_bytes());
            message.push(0);
        }
        message.push(u8::from(self.secure));
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ServerInfo<'static> {
        ServerInfo {
            spawn_count: 2,
            encoded_world_crc: 0x44332211,
            client_dll_md5: [0x42; 16],
            max_players: 2,
            player_index: 1,
            game_type: 1,
            game_dir: "cstrike",
            hostname: "test",
            world_model: "maps/test.bsp",
            map_cycle: "",
            secure: false,
        }
    }

    #[test]
    fn serializes_known_serverinfo_fields_in_wire_order() {
        let mut expected = vec![11, 48, 0, 0, 0, 2, 0, 0, 0, 0x11, 0x22, 0x33, 0x44];
        expected.extend_from_slice(&[0x42; 16]);
        expected.extend_from_slice(b"\x02\x01\x01cstrike\0test\0maps/test.bsp\0\0\0");
        assert_eq!(info().encode().unwrap(), expected);
    }

    #[test]
    fn rejects_invalid_counts_and_unsafe_strings() {
        let mut value = info();
        value.max_players = 0;
        assert!(value.encode().is_err());
        value.max_players = 33;
        assert!(value.encode().is_err());
        value.max_players = 1;
        assert!(value.encode().is_err());
        value.player_index = 0;
        value.game_type = 2;
        assert!(value.encode().is_err());
        value.game_type = 1;
        value.hostname = "broken\0name";
        assert!(value.encode().is_err());
        value.hostname = "ok";
        value.world_model = "";
        assert!(value.encode().is_err());
        let long = "x".repeat(256);
        value.world_model = &long;
        assert!(value.encode().is_err());
    }
}
