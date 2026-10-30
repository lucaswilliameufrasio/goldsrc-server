//! Bounded GoldSrc svc_serverinfo encoder. Not wired to live sign-on yet.
use std::io;

/// Explicitly supplied world/client metadata; no implicit map or executable data.
pub struct ServerInfo<'a> {
    pub spawn_count: u32,
    /// GoldSrc's unfinalized multiplayer map CRC, before the player transform.
    pub world_crc: u32,
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
        message
            .extend_from_slice(&encode_world_crc(self.world_crc, self.player_index).to_le_bytes());
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

/// Encode the raw world CRC using GoldSrc's COM_Munge3, with key (-1 - slot) & 255.
/// This is protocol obfuscation, not a cryptographic integrity guarantee.
fn encode_world_crc(world_crc: u32, player_index: u8) -> u32 {
    const TABLE: [u8; 4] = [0x20, 0x07, 0x13, 0x61];
    let key = u32::from(u8::MAX - player_index);
    let mut bytes = (world_crc ^ !key).swap_bytes().to_le_bytes();
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= 0xa5 | ((index as u8) << index) | index as u8 | TABLE[index];
    }
    u32::from_le_bytes(bytes) ^ key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ServerInfo<'static> {
        ServerInfo {
            spawn_count: 2,
            world_crc: 0x451b6da9,
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
        let mut expected = vec![11, 48, 0, 0, 0, 2, 0, 0, 0, 0xe1, 0x43, 0x2d, 0x57];
        expected.extend_from_slice(&[0x42; 16]);
        expected.extend_from_slice(b"\x02\x01\x01cstrike\0test\0maps/test.bsp\0\0\0");
        assert_eq!(info().encode().unwrap(), expected);
    }

    #[test]
    fn matches_official_hlds_munge3_for_multiple_player_slots() {
        assert_eq!(encode_world_crc(0x451b6da9, 0), 0x562d43e0);
        assert_eq!(encode_world_crc(0x451b6da9, 1), 0x572d43e1);
        assert_eq!(encode_world_crc(0x451b6da9, 31), 0x492d43ff);
    }

    #[test]
    fn encoding_serverinfo_uses_player_slot_and_real_reference_inputs() {
        let mut value = info();
        value.max_players = 32;
        value.client_dll_md5 = [0; 16]; // observed clientcrccheck=false
        for (slot, encoded) in [(0, 0x562d43e0_u32), (1, 0x572d43e1), (31, 0x492d43ff)] {
            value.player_index = slot;
            let wire = value.encode().unwrap();
            assert_eq!(&wire[9..13], &encoded.to_le_bytes());
            assert_eq!(&wire[13..29], &[0; 16]);
            assert_eq!(wire[30], slot);
        }
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
