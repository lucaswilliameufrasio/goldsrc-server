//! Bounded first-sign-on diagnostic. Not a world/game simulation.
use crate::fragments::{Ack, FragmentQueue};
use crate::resources::{ManifestEntry, ResourceNegotiation};
use crate::serverinfo::ServerInfo;
use crate::signon::NewResponse;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const RELIABLE: u32 = 1 << 31;
const FRAGMENT: u32 = 1 << 30;
const SEQUENCE_MASK: u32 = FRAGMENT - 1;
const MAX_PACKET: usize = 1400;
const CAPACITY: usize = 32;
const WAIT_NEW: Duration = Duration::from_secs(15);
const CLOSE_WAIT: Duration = Duration::from_secs(5);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const SIGNON_UNAVAILABLE: &[u8] =
    b"Authenticated connection accepted; development world sign-on is not implemented.";

// Functional protocol constants used by the GoldSrc Munge2 byte transform.
const TABLE: [u8; 16] = [
    0x05, 0x61, 0x7a, 0xed, 0x1b, 0xca, 0x0d, 0x9b, 0x4a, 0xf1, 0x64, 0xc7, 0xb5, 0x8e, 0xdf, 0xa0,
];

fn mask(word: usize, byte: usize, key: u8) -> u8 {
    let lookup = 0xa5 | ((byte as u8) << byte) | byte as u8 | TABLE[(word + byte) & 15];
    0xff ^ lookup ^ if byte == 0 || byte == 3 { key } else { 0 }
}

fn munge(data: &mut [u8], key: u8, encode: bool) {
    for (word, chunk) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let original = [chunk[0], chunk[1], chunk[2], chunk[3]];
        for byte in 0..4 {
            if encode {
                chunk[byte] = original[3 - byte] ^ mask(word, byte, key);
            } else {
                chunk[3 - byte] = original[byte] ^ mask(word, byte, key);
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum Command {
    Nop,
    New,
    Sendres,
    Disconnect,
}

fn command(payload: &[u8]) -> Result<Command, ()> {
    let mut cursor = 0;
    let mut result = Command::Nop;
    while cursor < payload.len() {
        let opcode = payload[cursor];
        cursor += 1;
        match opcode {
            1 => {}
            3 => {
                let rest = &payload[cursor..];
                let end = rest.iter().position(|byte| *byte == 0).ok_or(())?;
                if end > 256 {
                    return Err(());
                }
                match rest[..end].trim_ascii() {
                    b"new" if result == Command::Nop => result = Command::New,
                    b"dropclient" | b"disconnect" => result = Command::Disconnect,
                    b"new" if result == Command::Disconnect => {}
                    b"sendres" if result == Command::Nop => result = Command::Sendres,
                    _ => return Err(()),
                }
                cursor += end + 1;
            }
            _ => return Err(()),
        }
    }
    Ok(result)
}

enum Phase {
    AwaitingNew,
    Prepared {
        new_message: Vec<u8>,
        resources: ResourceNegotiation,
        manifest: Vec<u8>,
    },
    Fragments {
        started: Instant,
        queue: FragmentQueue,
        send_pending: bool,
        next: FragmentNext,
    },
    AwaitingSendres {
        started: Instant,
        resources: ResourceNegotiation,
        manifest: Vec<u8>,
    },
    AwaitingConsistency {
        started: Instant,
    },
    Closing {
        started: Instant,
        payload: Vec<u8>,
        last_reliable: Option<u32>,
        reliable_bit: bool,
    },
}

enum FragmentNext {
    Close,
    Signon {
        resources: ResourceNegotiation,
        manifest: Vec<u8>,
    },
}

pub struct Update {
    pub packet: Option<Vec<u8>>,
    pub outcome: &'static str,
    pub finished: bool,
}

impl Update {
    fn status(outcome: &'static str, finished: bool) -> Self {
        Self {
            packet: None,
            outcome,
            finished,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SessionIdentity {
    userid: u32,
    slot: u8,
}

struct Channel {
    identity: SessionIdentity,
    acceptance: Vec<u8>,
    accepted: Instant,
    last_send: Instant,
    incoming: u32,
    incoming_reliable: bool,
    outgoing: u32,
    acknowledged: u32,
    reliable_ack: bool,
    outgoing_reliable: bool,
    resources: Option<ResourceNegotiation>,
    phase: Phase,
}

impl Channel {
    fn new(peer: SocketAddr, identity: SessionIdentity, now: Instant) -> Self {
        let mut acceptance = vec![0xff; 4];
        acceptance
            .extend_from_slice(format!("B {} \"{peer}\" 0 10211\0", identity.userid).as_bytes());
        Self {
            identity,
            acceptance,
            accepted: now,
            last_send: now,
            incoming: 0,
            incoming_reliable: false,
            outgoing: 1,
            acknowledged: 0,
            reliable_ack: false,
            outgoing_reliable: false,
            resources: None,
            phase: Phase::AwaitingNew,
        }
    }

    fn start_close(&mut self, now: Instant, reason: &[u8]) {
        if !matches!(self.phase, Phase::Closing { .. }) {
            let mut payload = vec![2]; // svc_disconnect, followed by a NUL string.
            payload.extend_from_slice(reason);
            payload.push(0);
            self.outgoing_reliable = !self.outgoing_reliable;
            self.phase = Phase::Closing {
                started: now,
                payload,
                last_reliable: None,
                reliable_bit: self.outgoing_reliable,
            };
        }
    }

    fn transmit(&mut self, now: Instant, outcome: &'static str) -> Update {
        if self.outgoing > SEQUENCE_MASK {
            return Update::status("netchan_sequence_exhausted", true);
        }
        let mut reliable = false;
        let mut fragment = false;
        let mut payload = Vec::new();
        if let Phase::Fragments {
            queue,
            send_pending,
            ..
        } = &mut self.phase
            && *send_pending
        {
            match queue.transmit(self.outgoing) {
                Ok(bytes) => payload = bytes,
                Err(_) => return Update::status("netchan_fragment_error", true),
            }
            self.outgoing_reliable = queue.reliable_bit();
            *send_pending = false;
            reliable = true;
            fragment = true;
        }
        if let Phase::Closing {
            payload: pending,
            last_reliable,
            reliable_bit,
            ..
        } = &mut self.phase
            && last_reliable.is_none_or(|sequence| {
                self.acknowledged > sequence && self.reliable_ack != *reliable_bit
            })
        {
            reliable = true;
            payload.extend_from_slice(pending);
            *last_reliable = Some(self.outgoing);
        }
        payload.resize(payload.len().max(8), 1); // svc_nop padding, 16-byte datagram minimum.
        let sequence = self.outgoing;
        self.outgoing += 1;
        let mut packet =
            (sequence | if reliable { RELIABLE } else { 0 } | if fragment { FRAGMENT } else { 0 })
                .to_le_bytes()
                .to_vec();
        packet.extend_from_slice(
            &(self.incoming | if self.incoming_reliable { RELIABLE } else { 0 }).to_le_bytes(),
        );
        munge(&mut payload, sequence as u8, true);
        packet.extend_from_slice(&payload);
        self.last_send = now;
        Update {
            packet: Some(packet),
            outcome,
            finished: false,
        }
    }

    fn receive(&mut self, packet: &[u8], now: Instant) -> Update {
        if !(8..=MAX_PACKET).contains(&packet.len()) || packet.starts_with(&[0xff; 4]) {
            return Update::status("netchan_invalid", false);
        }
        let raw_sequence = u32::from_le_bytes(packet[..4].try_into().unwrap());
        let raw_ack = u32::from_le_bytes(packet[4..8].try_into().unwrap());
        let sequence = raw_sequence & SEQUENCE_MASK;
        let ack = raw_ack & SEQUENCE_MASK;
        if raw_ack & FRAGMENT != 0 || ack >= self.outgoing || ack < self.acknowledged {
            return Update::status("netchan_invalid", false);
        }
        if sequence <= self.incoming {
            return Update::status("netchan_duplicate_or_reordered", false);
        }
        if sequence - self.incoming > 4096 {
            return Update::status("netchan_invalid", false);
        }
        if raw_sequence & FRAGMENT != 0 {
            self.start_close(
                now,
                b"Development channel fragmentation is not implemented.",
            );
            return self.transmit(now, "netchan_fragment_unsupported");
        }
        self.incoming = sequence;
        self.acknowledged = ack;
        self.reliable_ack = raw_ack & RELIABLE != 0;
        if raw_sequence & RELIABLE != 0 {
            self.incoming_reliable = !self.incoming_reliable;
        }
        if let Phase::Closing {
            last_reliable: Some(sent),
            reliable_bit,
            ..
        } = self.phase
            && self.reliable_ack == reliable_bit
            && ack >= sent
        {
            return Update::status("netchan_disconnect_acked", true);
        }
        if let Phase::Fragments {
            queue,
            send_pending,
            next,
            ..
        } = &mut self.phase
        {
            match queue.acknowledge(ack, self.reliable_ack) {
                Ack::Ignored => {}
                Ack::Resend | Ack::Advanced => *send_pending = true,
                Ack::Complete => {
                    self.outgoing_reliable = queue.reliable_bit();
                    let next = std::mem::replace(next, FragmentNext::Close);
                    if let FragmentNext::Signon {
                        resources,
                        manifest,
                    } = next
                    {
                        self.phase = Phase::AwaitingSendres {
                            started: now,
                            resources,
                            manifest,
                        };
                    } else if self
                        .resources
                        .as_ref()
                        .is_some_and(ResourceNegotiation::expects_consistency)
                    {
                        self.phase = Phase::AwaitingConsistency { started: now };
                    } else {
                        self.start_close(now, SIGNON_UNAVAILABLE);
                        return self.transmit(now, "netchan_fragments_complete");
                    }
                }
            }
        }
        let mut payload = packet[8..].to_vec();
        munge(&mut payload, sequence as u8, false);
        if matches!(self.phase, Phase::AwaitingConsistency { .. }) && payload.first() == Some(&7) {
            let frame = &payload[1..];
            let valid_tail = if frame.len() >= 2 {
                let length = i16::from_le_bytes([frame[0], frame[1]]);
                length > 0
                    && (length as usize + 2) <= frame.len()
                    && frame[length as usize + 2..].iter().all(|byte| *byte == 1)
            } else {
                false
            };
            let valid = valid_tail
                && self
                    .resources
                    .as_mut()
                    .is_some_and(|state| state.receive_consistency(frame).is_ok());
            if valid {
                self.start_close(now, SIGNON_UNAVAILABLE);
                return self.transmit(now, "netchan_consistency_valid_resources_unavailable");
            }
            self.start_close(now, b"Invalid or inconsistent resource response.");
            return self.transmit(now, "netchan_consistency_rejected");
        }
        match command(&payload) {
            Ok(Command::Disconnect) => Update::status("netchan_client_disconnect", true),
            Ok(Command::New) => {
                if matches!(self.phase, Phase::Prepared { .. }) {
                    let phase = std::mem::replace(&mut self.phase, Phase::AwaitingNew);
                    if let Phase::Prepared {
                        new_message,
                        resources,
                        manifest,
                    } = phase
                    {
                        match FragmentQueue::new(new_message, 1024, self.outgoing_reliable) {
                            Ok(queue) => {
                                self.phase = Phase::Fragments {
                                    started: now,
                                    queue,
                                    send_pending: true,
                                    next: FragmentNext::Signon {
                                        resources,
                                        manifest,
                                    },
                                };
                                return self.transmit(now, "netchan_signon_start");
                            }
                            Err(_) => {
                                self.start_close(now, SIGNON_UNAVAILABLE);
                                return self.transmit(now, "netchan_signon_invalid");
                            }
                        }
                    }
                }
                self.start_close(now, SIGNON_UNAVAILABLE);
                self.transmit(now, "netchan_new")
            }
            Ok(Command::Sendres) => {
                if matches!(self.phase, Phase::AwaitingSendres { .. }) {
                    let phase = std::mem::replace(&mut self.phase, Phase::AwaitingNew);
                    if let Phase::AwaitingSendres {
                        resources,
                        manifest,
                        ..
                    } = phase
                    {
                        match FragmentQueue::new(manifest, 1024, self.outgoing_reliable) {
                            Ok(queue) => {
                                self.resources = Some(resources);
                                self.phase = Phase::Fragments {
                                    started: now,
                                    queue,
                                    send_pending: true,
                                    next: FragmentNext::Close,
                                };
                                return self.transmit(now, "netchan_resource_start");
                            }
                            Err(_) => {
                                self.start_close(now, SIGNON_UNAVAILABLE);
                                return self.transmit(now, "netchan_resource_invalid");
                            }
                        }
                    }
                }
                self.start_close(now, b"Unexpected resource request.");
                self.transmit(now, "netchan_sendres_unexpected")
            }
            Ok(Command::Nop) => self.transmit(now, "netchan_ack"),
            Err(()) => {
                self.start_close(
                    now,
                    b"Development channel received an unsupported or malformed message.",
                );
                self.transmit(now, "netchan_payload_unsupported")
            }
        }
    }

    fn tick(&mut self, now: Instant) -> Option<Update> {
        match self.phase {
            Phase::AwaitingNew | Phase::Prepared { .. }
                if now.saturating_duration_since(self.accepted) >= WAIT_NEW =>
            {
                Some(Update::status("netchan_timeout", true))
            }
            Phase::Closing { started, .. }
                if now.saturating_duration_since(started) >= CLOSE_WAIT =>
            {
                Some(Update::status("netchan_timeout", true))
            }
            Phase::Fragments { started, .. }
                if now.saturating_duration_since(started) >= WAIT_NEW =>
            {
                Some(Update::status("netchan_fragment_timeout", true))
            }
            Phase::AwaitingConsistency { started }
                if now.saturating_duration_since(started) >= WAIT_NEW =>
            {
                Some(Update::status("netchan_consistency_timeout", true))
            }
            Phase::AwaitingSendres { started, .. }
                if now.saturating_duration_since(started) >= WAIT_NEW =>
            {
                Some(Update::status("netchan_sendres_timeout", true))
            }
            Phase::Fragments { .. }
                if now.saturating_duration_since(self.last_send) >= PROBE_INTERVAL =>
            {
                Some(self.transmit(now, "netchan_fragment_probe"))
            }
            Phase::Closing { .. }
                if now.saturating_duration_since(self.last_send) >= PROBE_INTERVAL =>
            {
                Some(self.transmit(now, "netchan_close_probe"))
            }
            _ => None,
        }
    }
}

pub struct Sessions {
    channels: HashMap<SocketAddr, Channel>,
    next_userid: u32,
}

impl Sessions {
    /// Prepare an explicitly supplied world snapshot for one approved session.
    /// No runtime producer calls this yet; this does not authenticate the peer.
    #[allow(dead_code)]
    fn prepare_signon(
        &mut self,
        peer: SocketAddr,
        mut response: NewResponse<'_>,
        entries: &[ManifestEntry<'_>],
    ) -> Result<(), &'static str> {
        let channel = self.channels.get_mut(&peer).ok_or("netchan_unknown_peer")?;
        if !matches!(channel.phase, Phase::AwaitingNew) {
            return Err("netchan_busy");
        }
        response.serverinfo.info.player_index = channel.identity.slot;
        let (resources, manifest) =
            ResourceNegotiation::prepare(response.serverinfo.info.spawn_count, entries)
                .map_err(|_| "netchan_invalid_manifest")?;
        let new_message = response.encode().map_err(|_| "netchan_invalid_signon")?;
        FragmentQueue::new(new_message.clone(), 1024, channel.outgoing_reliable)
            .map_err(|_| "netchan_invalid_signon")?;
        FragmentQueue::new(manifest.clone(), 1024, channel.outgoing_reliable)
            .map_err(|_| "netchan_invalid_manifest")?;
        channel.phase = Phase::Prepared {
            new_message,
            resources,
            manifest,
        };
        Ok(())
    }
    /// Prepare serverinfo for an approved session. Does not send a packet.
    #[allow(dead_code)]
    fn prepare_serverinfo(
        &self,
        peer: SocketAddr,
        mut info: ServerInfo<'_>,
    ) -> std::io::Result<Vec<u8>> {
        let channel = self.channels.get(&peer).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "unknown approved session")
        })?;
        if !matches!(channel.phase, Phase::AwaitingNew) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session is not awaiting new",
            ));
        }
        info.player_index = channel.identity.slot;
        info.encode()
    }

    /// Prepared for a future sign-on producer; requires an already approved peer.
    #[allow(dead_code)]
    pub fn queue_resource_manifest(
        &mut self,
        peer: SocketAddr,
        spawn_count: u32,
        entries: &[ManifestEntry<'_>],
        now: Instant,
    ) -> Result<Vec<u8>, &'static str> {
        let (resources, message) = ResourceNegotiation::prepare(spawn_count, entries)
            .map_err(|_| "netchan_invalid_manifest")?;
        let channel = self.channels.get_mut(&peer).ok_or("netchan_unknown_peer")?;
        if !matches!(channel.phase, Phase::AwaitingNew) {
            return Err("netchan_busy");
        }
        let queue = FragmentQueue::new(message, 1024, channel.outgoing_reliable)
            .map_err(|_| "netchan_invalid_manifest")?;
        channel.resources = Some(resources);
        channel.phase = Phase::Fragments {
            started: now,
            queue,
            send_pending: true,
            next: FragmentNext::Close,
        };
        channel
            .transmit(now, "netchan_resource_start")
            .packet
            .ok_or("netchan_fragment_error")
    }
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            next_userid: 1,
        }
    }

    // Production caller must have a positive Steam approval, never a basic ticket result.
    pub fn approve(&mut self, peer: SocketAddr, now: Instant) -> Result<Vec<u8>, &'static str> {
        if self.channels.len() >= CAPACITY || self.channels.contains_key(&peer) {
            return Err("connection_capacity");
        }
        let slot = (0..CAPACITY)
            .find(|slot| {
                !self
                    .channels
                    .values()
                    .any(|channel| usize::from(channel.identity.slot) == *slot)
            })
            .ok_or("connection_capacity")? as u8;
        let userid = self.next_userid;
        self.next_userid = userid.checked_add(1).ok_or("connection_capacity")?;
        let channel = Channel::new(peer, SessionIdentity { userid, slot }, now);
        let packet = channel.acceptance.clone();
        self.channels.insert(peer, channel);
        Ok(packet)
    }

    pub fn contains(&self, peer: SocketAddr) -> bool {
        self.channels.contains_key(&peer)
    }

    /// Reserved for a future validated sign-on producer; no live caller exists yet.
    #[allow(dead_code)]
    pub fn queue_fragments(
        &mut self,
        peer: SocketAddr,
        message: Vec<u8>,
        now: Instant,
    ) -> Result<Vec<u8>, &'static str> {
        let channel = self.channels.get_mut(&peer).ok_or("netchan_unknown_peer")?;
        if !matches!(channel.phase, Phase::AwaitingNew) {
            return Err("netchan_busy");
        }
        let queue = FragmentQueue::new(message, 1024, channel.outgoing_reliable)
            .map_err(|_| "netchan_invalid_fragments")?;
        channel.phase = Phase::Fragments {
            started: now,
            queue,
            send_pending: true,
            next: FragmentNext::Close,
        };
        channel
            .transmit(now, "netchan_fragment_start")
            .packet
            .ok_or("netchan_fragment_error")
    }

    pub fn retry_acceptance(&mut self, peer: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        let channel = self.channels.get_mut(&peer)?;
        if matches!(channel.phase, Phase::AwaitingNew | Phase::Prepared { .. })
            && now.saturating_duration_since(channel.last_send) >= PROBE_INTERVAL
        {
            channel.last_send = now;
            return Some(channel.acceptance.clone());
        }
        None
    }

    pub fn receive(&mut self, peer: SocketAddr, packet: &[u8], now: Instant) -> Option<Update> {
        let channel = self.channels.get_mut(&peer)?;
        let update = channel.receive(packet, now);
        if update.finished {
            self.channels.remove(&peer);
        }
        Some(update)
    }

    pub fn revoke(&mut self, peer: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        let mut channel = self.channels.remove(&peer)?;
        channel.start_close(now, b"Steam authentication revoked.");
        channel.transmit(now, "netchan_auth_revoked").packet
    }

    pub fn tick(&mut self, now: Instant) -> Vec<(SocketAddr, Update)> {
        let mut updates = Vec::new();
        for (peer, channel) in &mut self.channels {
            if let Some(update) = channel.tick(now) {
                updates.push((*peer, update));
            }
        }
        for (peer, update) in &updates {
            if update.finished {
                self.channels.remove(peer);
            }
        }
        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires local HLDS BSP, delta and manifest captures"]
    fn prepared_sequence_with_local_world_inputs() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = std::path::Path::new(&root);
        let (_, crc) = steam_server_query_poc::map::inspect(
            &root.join("reference-hlds/cstrike/maps/de_dust2.bsp"),
        )
        .unwrap();
        let delta = steam_server_query_poc::delta::load(
            &root.join("reference-hlds/cstrike/delta.lst"),
            &root.join("hlds-delta-fields-build10211.tsv"),
        )
        .unwrap();
        let snapshot = crate::resources::ManifestSnapshot::load(
            &root.join("hlds-manifest-10211-v2.tsv"),
            &root.join("hlds-consistency-metadata-10211.tsv"),
            1,
        )
        .unwrap();
        let entries = snapshot.entries();
        let expected_manifest =
            std::fs::read(root.join("hlds-manifest-consistency-10211.bin")).unwrap();
        let mut signon = crate::signon::tests::synthetic(&delta.bytes);
        signon.info.world_crc = crc;
        let response = NewResponse {
            serverinfo: signon,
            user_messages: &[],
            full_server_info: "\\hostname\\test",
            allow_shaders: false,
            allow_autoaim: false,
            clients: &[],
        };
        let expected_signon = response.encode().unwrap();
        let now = Instant::now();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), now).unwrap();
        sessions
            .prepare_signon(peer(1), response, &entries)
            .unwrap();
        let mut outgoing = sessions
            .receive(peer(1), &packet(1, 0, b"\x03new\0"), now)
            .unwrap()
            .packet
            .unwrap();
        let mut incoming = 2_u32;
        let mut reliable = false;
        for stage in 0..2 {
            let mut assembled = Vec::new();
            let mut index = 1_u32;
            loop {
                let raw = u32::from_le_bytes(outgoing[..4].try_into().unwrap());
                assert_eq!(raw & (RELIABLE | FRAGMENT), RELIABLE | FRAGMENT);
                let mut body = outgoing[8..].to_vec();
                munge(&mut body, raw as u8, false);
                let id = u32::from_le_bytes(body[1..5].try_into().unwrap());
                assert_eq!(id >> 16, index);
                let size = usize::from(u16::from_le_bytes(body[7..9].try_into().unwrap()));
                assembled.extend_from_slice(&body[10..10 + size]);
                reliable = !reliable;
                let ack = (raw & SEQUENCE_MASK) | if reliable { RELIABLE } else { 0 };
                let last = index == id & 0xffff;
                let payload: &[u8] = if stage == 0 && last {
                    b"\x03sendres\0"
                } else {
                    &[1]
                };
                let update = sessions
                    .receive(peer(1), &packet(incoming, ack, payload), now)
                    .unwrap();
                incoming += 1;
                outgoing = update.packet.unwrap();
                if last {
                    if stage == 0 {
                        assert_eq!(update.outcome, "netchan_resource_start");
                    } else {
                        assert_eq!(update.outcome, "netchan_ack");
                    }
                    break;
                }
                index += 1;
            }
            if stage == 0 {
                assert_eq!(assembled, expected_signon);
            } else {
                assert_eq!(assembled, expected_manifest);
                assert_eq!(index, 19);
            }
        }
        assert_eq!(
            sessions.tick(now + WAIT_NEW)[0].1.outcome,
            "netchan_consistency_timeout"
        );
    }

    fn response_for_sequence() -> NewResponse<'static> {
        NewResponse {
            serverinfo: crate::signon::tests::synthetic(&[14]),
            user_messages: &[],
            full_server_info: "\\hostname\\test",
            allow_shaders: false,
            allow_autoaim: false,
            clients: &[],
        }
    }

    fn resource_for_sequence() -> [ManifestEntry<'static>; 1] {
        [ManifestEntry {
            resource: crate::resources::Resource {
                kind: 0,
                filename: "test.wav",
                index: 1,
                download_size: 1,
                flags: 0,
                reserved: [0; 32],
            },
            consistency_hash: Some(0x12345678_u32.to_le_bytes()),
        }]
    }

    #[test]
    fn prepared_new_then_sendres_then_consistency() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        assert!(
            sessions
                .prepare_signon(peer(1), response_for_sequence(), &resource_for_sequence())
                .is_err()
        );
        sessions.approve(peer(1), now).unwrap();
        sessions
            .prepare_signon(peer(1), response_for_sequence(), &resource_for_sequence())
            .unwrap();
        let first = sessions
            .receive(peer(1), &packet(1, 0, b"\x03new\0"), now)
            .unwrap();
        assert_eq!(first.outcome, "netchan_signon_start");
        let first = first.packet.unwrap();
        assert_eq!(
            u32::from_le_bytes(first[..4].try_into().unwrap()),
            RELIABLE | FRAGMENT | 1
        );
        let mut body = first[8..].to_vec();
        munge(&mut body, 1, false);
        let size = usize::from(u16::from_le_bytes(body[7..9].try_into().unwrap()));
        assert_eq!(
            &body[10..10 + size],
            response_for_sequence().encode().unwrap()
        );
        let next = sessions
            .receive(peer(1), &packet(2, RELIABLE | 1, b"\x03sendres\0"), now)
            .unwrap();
        assert_eq!(next.outcome, "netchan_resource_start");
        let next = next.packet.unwrap();
        assert_eq!(
            u32::from_le_bytes(next[..4].try_into().unwrap()),
            RELIABLE | FRAGMENT | 2
        );
        let result = sessions
            .receive(
                peer(1),
                &packet(3, 2, &[7, 6, 0, 0x8b, 0xd7, 0x50, 0, 0x46, 2]),
                now,
            )
            .unwrap();
        assert_eq!(
            result.outcome,
            "netchan_consistency_valid_resources_unavailable"
        );
        assert!(!result.finished);
    }

    #[test]
    fn prepared_sequence_rejects_early_sendres_and_invalid_plan() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), now).unwrap();
        let mut invalid = response_for_sequence();
        invalid.serverinfo.info.max_players = 0;
        assert!(
            sessions
                .prepare_signon(peer(1), invalid, &resource_for_sequence())
                .is_err()
        );
        sessions
            .prepare_signon(peer(1), response_for_sequence(), &resource_for_sequence())
            .unwrap();
        let result = sessions
            .receive(peer(1), &packet(1, 0, b"\x03sendres\0"), now)
            .unwrap();
        assert_eq!(result.outcome, "netchan_sendres_unexpected");
        assert!(
            sessions
                .prepare_signon(peer(1), response_for_sequence(), &resource_for_sequence())
                .is_err()
        );
        sessions.approve(peer(2), now).unwrap();
        sessions
            .prepare_signon(peer(2), response_for_sequence(), &resource_for_sequence())
            .unwrap();
        sessions
            .receive(peer(2), &packet(1, 0, b"\x03new\0"), now)
            .unwrap();
        let result = sessions
            .receive(peer(2), &packet(2, 0, b"\x03sendres\0"), now)
            .unwrap();
        assert_eq!(result.outcome, "netchan_sendres_unexpected");
    }

    fn serverinfo_for_test(world_crc: u32, max_players: u8) -> ServerInfo<'static> {
        ServerInfo {
            spawn_count: 1,
            world_crc,
            client_dll_md5: [0; 16],
            max_players,
            player_index: 31,
            game_type: 1,
            game_dir: "cstrike",
            hostname: "test",
            world_model: "maps/test.bsp",
            map_cycle: "",
            secure: false,
        }
    }

    #[test]
    fn serverinfo_preparation_binds_slot_to_approved_session() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        assert!(
            sessions
                .prepare_serverinfo(peer(1), serverinfo_for_test(0x451b6da9, 2))
                .is_err()
        );
        sessions.approve(peer(1), now).unwrap();
        sessions.approve(peer(2), now).unwrap();
        let first = sessions
            .prepare_serverinfo(peer(1), serverinfo_for_test(0x451b6da9, 2))
            .unwrap();
        let second = sessions
            .prepare_serverinfo(peer(2), serverinfo_for_test(0x451b6da9, 2))
            .unwrap();
        assert_eq!(&first[9..13], &0x562d43e0_u32.to_le_bytes());
        assert_eq!(&second[9..13], &0x572d43e1_u32.to_le_bytes());
        assert_eq!(first[30], 0);
        assert_eq!(second[30], 1);
        assert!(
            sessions
                .prepare_serverinfo(peer(2), serverinfo_for_test(0x451b6da9, 1))
                .is_err()
        );
        sessions
            .receive(peer(1), &packet(RELIABLE | 1, 0, b"\x03new\0"), now)
            .unwrap();
        assert!(
            sessions
                .prepare_serverinfo(peer(1), serverinfo_for_test(0x451b6da9, 2))
                .is_err()
        );
        sessions.revoke(peer(2), now).unwrap();
        assert!(
            sessions
                .prepare_serverinfo(peer(2), serverinfo_for_test(0x451b6da9, 2))
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires local HLDS BSP reference"]
    fn serverinfo_uses_crc_from_local_bsp_and_session_slot() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = std::path::Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let (_, crc) = steam_server_query_poc::map::inspect(&path).unwrap();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), Instant::now()).unwrap();
        let encoded = sessions
            .prepare_serverinfo(peer(1), serverinfo_for_test(crc, 2))
            .unwrap();
        assert_eq!(&encoded[9..13], &0x562d43e0_u32.to_le_bytes());
        assert_eq!(encoded[30], 0);
    }

    #[test]
    fn approved_session_slots_are_unique_and_released_on_cleanup() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), now).unwrap();
        sessions.approve(peer(2), now).unwrap();
        assert_eq!(
            sessions.channels[&peer(1)].identity,
            SessionIdentity { userid: 1, slot: 0 }
        );
        assert_eq!(
            sessions.channels[&peer(2)].identity,
            SessionIdentity { userid: 2, slot: 1 }
        );
        assert!(sessions.approve(peer(1), now).is_err());
        sessions
            .receive(peer(1), &packet(RELIABLE | 1, 0, b"\x03new\0"), now)
            .unwrap();
        sessions.approve(peer(3), now).unwrap();
        assert_eq!(sessions.channels[&peer(3)].identity.slot, 2);
        sessions.revoke(peer(1), now).unwrap();
        sessions.approve(peer(4), now).unwrap();
        assert_eq!(
            sessions.channels[&peer(4)].identity,
            SessionIdentity { userid: 4, slot: 0 }
        );
        sessions.tick(now + WAIT_NEW);
        assert!(sessions.channels.is_empty());
        sessions.approve(peer(5), now + WAIT_NEW).unwrap();
        assert_eq!(
            sessions.channels[&peer(5)].identity,
            SessionIdentity { userid: 5, slot: 0 }
        );
    }

    #[test]
    #[ignore = "requires local HLDS manifest and consistency captures"]
    fn reference_manifest_fragments_reassemble_and_wait_for_consistency() {
        use crate::resources::Resource;
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = std::path::Path::new(&root);
        let manifest = std::fs::read_to_string(root.join("hlds-manifest-10211-v2.tsv")).unwrap();
        let metadata =
            std::fs::read_to_string(root.join("hlds-consistency-metadata-10211.tsv")).unwrap();
        let reference = std::fs::read(root.join("hlds-manifest-consistency-10211.bin")).unwrap();
        fn hex<const N: usize>(value: &str) -> [u8; N] {
            assert_eq!(value.len(), 2 * N);
            let mut result = [0; N];
            for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                result[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            }
            result
        }
        let mut checks = std::collections::HashMap::new();
        for line in metadata.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 4);
            let position: usize = fields[0].parse().unwrap();
            assert!(
                checks
                    .insert(position, (hex::<4>(fields[1]), hex::<32>(fields[2])))
                    .is_none()
            );
        }
        assert_eq!(checks.len(), 62);
        let mut entries = Vec::new();
        for (position, line) in manifest.lines().enumerate() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 6);
            let flags: u8 = fields[4].parse().unwrap();
            assert_eq!(flags & !0x83, 0);
            let reserved = hex::<32>(fields[5]);
            let check = checks.remove(&position);
            assert_eq!(check.is_some(), flags & 0x80 != 0);
            let consistency_hash = check.map(|(hash, metadata_reserved)| {
                assert_eq!(metadata_reserved, reserved);
                hash
            });
            entries.push(ManifestEntry {
                resource: Resource {
                    kind: fields[1].parse().unwrap(),
                    filename: fields[0],
                    index: fields[2].parse().unwrap(),
                    download_size: fields[3].parse().unwrap(),
                    flags: flags & 3,
                    reserved,
                },
                consistency_hash,
            });
        }
        assert_eq!(entries.len(), 781);
        assert!(checks.is_empty());
        let now = Instant::now();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), now).unwrap();
        let mut outgoing = sessions
            .queue_resource_manifest(peer(1), 1, &entries, now)
            .unwrap();
        let mut reconstructed = Vec::new();
        let mut index = 1_u32;
        let mut reliable = false;
        loop {
            let raw = u32::from_le_bytes(outgoing[..4].try_into().unwrap());
            assert_eq!(raw & (RELIABLE | FRAGMENT), RELIABLE | FRAGMENT);
            let mut body = outgoing[8..].to_vec();
            munge(&mut body, raw as u8, false);
            assert_eq!(body[0], 1);
            let id = u32::from_le_bytes(body[1..5].try_into().unwrap());
            assert_eq!(id >> 16, index);
            assert_eq!(&body[5..7], &[0, 0]);
            assert_eq!(body[9], 0);
            let size = usize::from(u16::from_le_bytes(body[7..9].try_into().unwrap()));
            reconstructed.extend_from_slice(&body[10..10 + size]);
            reliable = !reliable;
            let ack = (raw & SEQUENCE_MASK) | if reliable { RELIABLE } else { 0 };
            let update = sessions
                .receive(peer(1), &packet(index, ack, &[1]), now)
                .unwrap();
            if index == id & 0xffff {
                assert_eq!(update.outcome, "netchan_ack");
                break;
            }
            outgoing = update.packet.unwrap();
            index += 1;
        }
        assert_eq!(index, 19);
        assert_eq!(reconstructed, reference);
        let updates = sessions.tick(now + WAIT_NEW);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].1.outcome, "netchan_consistency_timeout");
        assert!(!sessions.contains(peer(1)));
    }

    #[test]
    fn resource_fragment_ack_and_consistency_response_share_packet() {
        use crate::resources::Resource;
        let now = Instant::now();
        let entries = [ManifestEntry {
            resource: Resource {
                kind: 0,
                filename: "test.wav",
                index: 1,
                download_size: 1,
                flags: 0,
                reserved: [0; 32],
            },
            consistency_hash: Some(0x12345678_u32.to_le_bytes()),
        }];
        for (frame, expected) in [
            (
                &[7, 6, 0, 0x8b, 0xd7, 0x50, 0, 0x46, 2][..],
                "netchan_consistency_valid_resources_unavailable",
            ),
            (
                &[7, 6, 0, 0x8b, 0xd7, 0x50, 0, 0x47, 2][..],
                "netchan_consistency_rejected",
            ),
            (&[7, 0, 0][..], "netchan_consistency_rejected"),
        ] {
            let mut sessions = Sessions::new();
            assert!(
                sessions
                    .queue_resource_manifest(peer(1), 1, &entries, now)
                    .is_err()
            );
            sessions.approve(peer(1), now).unwrap();
            let first = sessions
                .queue_resource_manifest(peer(1), 1, &entries, now)
                .unwrap();
            assert_eq!(
                u32::from_le_bytes(first[..4].try_into().unwrap()),
                RELIABLE | FRAGMENT | 1
            );
            let response = sessions
                .receive(peer(1), &packet(1, RELIABLE | 1, frame), now)
                .unwrap();
            assert_eq!(response.outcome, expected);
            assert!(!response.finished);
        }
    }

    fn peer(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn packet(sequence: u32, ack: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = sequence.to_le_bytes().to_vec();
        bytes.extend_from_slice(&ack.to_le_bytes());
        let mut body = payload.to_vec();
        munge(&mut body, sequence as u8, true);
        bytes.extend_from_slice(&body);
        bytes
    }

    #[test]
    fn known_wire_vector_and_inverse() {
        let expected = [
            0x01, 0x00, 0x00, 0x80, 0, 0, 0, 0, 0x2c, 0x7d, 0x6e, 0x02, 0x1a, 0x01, 0x11, 0x41,
        ];
        assert_eq!(packet(RELIABLE | 1, 0, b"\x03new\0\x01\x01\x01"), expected);
        for len in 0..100 {
            let mut bytes: Vec<u8> = (0..len).map(|n| n as u8).collect();
            let original = bytes.clone();
            munge(&mut bytes, 234, true);
            munge(&mut bytes, 234, false);
            assert_eq!(bytes, original);
        }
    }

    #[test]
    fn only_approved_peer_gets_acceptance_and_new_gets_reliable_disconnect() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        assert!(
            sessions
                .receive(peer(1), &packet(1, 0, b"\x03new\0"), now)
                .is_none()
        );
        assert_eq!(
            sessions.approve(peer(1), now).unwrap(),
            b"\xff\xff\xff\xffB 1 \"127.0.0.1:1\" 0 10211\0"
        );
        assert!(
            sessions
                .receive(peer(2), &packet(1, 0, b"\x03new\0"), now)
                .is_none()
        );
        let update = sessions
            .receive(
                peer(1),
                &packet(RELIABLE | 1, 0, b"\x03new\0\x01\x01\x01"),
                now,
            )
            .unwrap();
        assert_eq!(update.outcome, "netchan_new");
        let response = update.packet.unwrap();
        assert_eq!(
            u32::from_le_bytes(response[..4].try_into().unwrap()),
            RELIABLE | 1
        );
        assert_eq!(
            u32::from_le_bytes(response[4..8].try_into().unwrap()),
            RELIABLE | 1
        );
        let mut body = response[8..].to_vec();
        munge(&mut body, 1, false);
        assert_eq!(body[0], 2);
        assert!(body[1..].starts_with(SIGNON_UNAVAILABLE));
        let ack = sessions
            .receive(peer(1), &packet(2, RELIABLE | 1, b"\x01"), now)
            .unwrap();
        assert_eq!(ack.outcome, "netchan_disconnect_acked");
        assert!(ack.finished);
        assert!(!sessions.contains(peer(1)));
    }

    #[test]
    fn lost_reliable_message_is_resent_after_later_negative_ack() {
        let now = Instant::now();
        let mut session = Channel::new(peer(1), SessionIdentity { userid: 1, slot: 0 }, now);
        session.receive(&packet(1, 0, b"\x03new\0"), now);
        let probe = session.tick(now + PROBE_INTERVAL).unwrap().packet.unwrap();
        assert_eq!(u32::from_le_bytes(probe[..4].try_into().unwrap()), 2);
        let resent = session
            .receive(&packet(2, 2, b"\x01"), now + PROBE_INTERVAL)
            .packet
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(resent[..4].try_into().unwrap()),
            RELIABLE | 3
        );
        assert!(
            session
                .receive(&packet(3, RELIABLE | 3, b"\x01"), now)
                .finished
        );
    }

    #[test]
    fn invalid_packets_do_not_advance_state_and_expire() {
        let now = Instant::now();
        let mut session = Channel::new(peer(1), SessionIdentity { userid: 1, slot: 0 }, now);
        for packet in [
            vec![],
            vec![0; 7],
            vec![0; MAX_PACKET + 1],
            packet(1, 1, b"\x01"),
            packet(4097, 0, b"\x01"),
            packet(1, FRAGMENT, b"\x01"),
        ] {
            assert_eq!(session.receive(&packet, now).outcome, "netchan_invalid");
            assert_eq!(session.incoming, 0);
        }
        assert_eq!(
            session.receive(&packet(1, 0, b"\x01"), now).outcome,
            "netchan_ack"
        );
        assert_eq!(
            session.receive(&packet(1, 0, b"\x01"), now).outcome,
            "netchan_duplicate_or_reordered"
        );
        assert!(session.tick(now + WAIT_NEW).unwrap().finished);
    }

    #[test]
    fn fragment_and_unsupported_commands_fail_closed() {
        let now = Instant::now();
        let mut channel = Channel::new(peer(1), SessionIdentity { userid: 1, slot: 0 }, now);
        assert_eq!(
            channel
                .receive(&packet(FRAGMENT | 1, 0, b"\x01"), now)
                .outcome,
            "netchan_fragment_unsupported"
        );
        assert!(channel.tick(now + CLOSE_WAIT).unwrap().finished);
        let mut channel = Channel::new(peer(1), SessionIdentity { userid: 1, slot: 0 }, now);
        assert_eq!(
            channel
                .receive(&packet(1, 0, b"\x03exec secret\0"), now)
                .outcome,
            "netchan_payload_unsupported"
        );
        assert_eq!(command(&[3, b'n', b'e', b'w']), Err(()));
        assert_eq!(
            command(
                &[3].into_iter()
                    .chain(vec![b'x'; 257])
                    .chain([0])
                    .collect::<Vec<_>>()
            ),
            Err(())
        );
    }

    #[test]
    fn capacity_timeout_revoke_and_retry_are_bounded() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        for i in 1..=CAPACITY as u16 {
            sessions.approve(peer(i), now).unwrap();
        }
        assert!(sessions.approve(peer(1000), now).is_err());
        assert!(sessions.retry_acceptance(peer(1), now).is_none());
        assert!(
            sessions
                .retry_acceptance(peer(1), now + PROBE_INTERVAL)
                .is_some()
        );
        assert!(sessions.revoke(peer(1), now).is_some());
        assert!(sessions.revoke(peer(1), now).is_none());
        assert_eq!(sessions.tick(now + WAIT_NEW).len(), CAPACITY - 1);
        assert!(sessions.approve(peer(1000), now + WAIT_NEW).is_ok());
    }

    #[test]
    fn fragments_use_channel_ack_loss_retry_and_reassemble() {
        let now = Instant::now();
        let source: Vec<u8> = (0..2300).map(|n| n as u8).collect();
        let mut sessions = Sessions::new();
        assert!(
            sessions
                .queue_fragments(peer(1), source.clone(), now)
                .is_err()
        );
        sessions.approve(peer(1), now).unwrap();
        let lost = sessions
            .queue_fragments(peer(1), source.clone(), now)
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(lost[..4].try_into().unwrap()),
            RELIABLE | FRAGMENT | 1
        );
        assert!(sessions.queue_fragments(peer(1), vec![1], now).is_err());
        let probe = sessions
            .tick(now + PROBE_INTERVAL)
            .pop()
            .unwrap()
            .1
            .packet
            .unwrap();
        assert_eq!(u32::from_le_bytes(probe[..4].try_into().unwrap()), 2);
        let resend = sessions
            .receive(peer(1), &packet(1, 2, b"\x01"), now)
            .unwrap()
            .packet
            .unwrap();
        let mut pieces = Vec::new();
        fn piece(packet: &[u8], index: u16, count: u16) -> Vec<u8> {
            let raw = u32::from_le_bytes(packet[..4].try_into().unwrap());
            assert_eq!(raw & (RELIABLE | FRAGMENT), RELIABLE | FRAGMENT);
            let mut payload = packet[8..].to_vec();
            munge(&mut payload, raw as u8, false);
            assert_eq!(payload[0], 1);
            assert_eq!(
                u32::from_le_bytes(payload[1..5].try_into().unwrap()),
                (u32::from(index) << 16) | u32::from(count)
            );
            assert_eq!(&payload[5..7], &[0, 0]);
            let size = usize::from(u16::from_le_bytes(payload[7..9].try_into().unwrap()));
            assert_eq!(payload[9], 0);
            payload[10..10 + size].to_vec()
        }
        pieces.extend(piece(&resend, 1, 3));
        let second = sessions
            .receive(peer(1), &packet(2, RELIABLE | 3, b"\x01"), now)
            .unwrap()
            .packet
            .unwrap();
        pieces.extend(piece(&second, 2, 3));
        let probe = sessions
            .tick(now + PROBE_INTERVAL)
            .pop()
            .unwrap()
            .1
            .packet
            .unwrap();
        assert_eq!(u32::from_le_bytes(probe[..4].try_into().unwrap()), 5);
        let second_retry = sessions
            .receive(peer(1), &packet(3, RELIABLE | 5, b"\x01"), now)
            .unwrap()
            .packet
            .unwrap();
        assert_eq!(piece(&second_retry, 2, 3), source[1024..2048]);
        let third = sessions
            .receive(peer(1), &packet(4, 6, b"\x01"), now)
            .unwrap()
            .packet
            .unwrap();
        pieces.extend(piece(&third, 3, 3));
        assert_eq!(pieces, source);
        let complete = sessions
            .receive(peer(1), &packet(5, RELIABLE | 7, b"\x01"), now)
            .unwrap();
        assert_eq!(complete.outcome, "netchan_fragments_complete");
        assert!(sessions.contains(peer(1)));
        let close = complete.packet.unwrap();
        assert_eq!(
            u32::from_le_bytes(close[..4].try_into().unwrap()),
            RELIABLE | 8
        );
        assert!(
            sessions
                .receive(peer(1), &packet(6, 8, b"\x01"), now)
                .unwrap()
                .finished
        );
    }

    #[test]
    fn fragment_transfer_expires_and_revoke_discards_pending_data() {
        let now = Instant::now();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), now).unwrap();
        sessions
            .queue_fragments(peer(1), vec![1; 2000], now)
            .unwrap();
        let updates = sessions.tick(now + WAIT_NEW);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].1.outcome, "netchan_fragment_timeout");
        assert!(!sessions.contains(peer(1)));
        sessions.approve(peer(1), now).unwrap();
        sessions
            .queue_fragments(peer(1), vec![1; 2000], now)
            .unwrap();
        let packet = sessions.revoke(peer(1), now).unwrap();
        assert_eq!(
            u32::from_le_bytes(packet[..4].try_into().unwrap()) & FRAGMENT,
            0
        );
        assert!(!sessions.contains(peer(1)));
    }

    #[test]
    #[ignore = "writes an opt-in synthetic packet fixture for isolated HLDS verification"]
    fn export_fragments_for_hlds() {
        use std::io::Write;
        let directory =
            std::env::var_os("GOLDSRC_FRAGMENT_FIXTURE_DIR").expect("missing fixture directory");
        let path = std::path::Path::new(&directory).join("rust-fragments-hlds.bin");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        let now = Instant::now();
        let source: Vec<u8> = (0..2300).map(|n| n as u8).collect();
        let mut sessions = Sessions::new();
        sessions.approve(peer(1), now).unwrap();
        let first = sessions.queue_fragments(peer(1), source, now).unwrap();
        let second = sessions
            .receive(peer(1), &packet(1, RELIABLE | 1, b"\x01"), now)
            .unwrap()
            .packet
            .unwrap();
        let third = sessions
            .receive(peer(1), &packet(2, 2, b"\x01"), now)
            .unwrap()
            .packet
            .unwrap();
        for data in [first, second, third] {
            file.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
            file.write_all(&data).unwrap();
        }
    }

    #[test]
    #[ignore = "requires local HLDS delta inputs"]
    fn prepared_signon_reassembles_via_fragment_channel() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let root = std::path::Path::new(&root);
        let delta = steam_server_query_poc::delta::load(
            &root.join("reference-hlds/cstrike/delta.lst"),
            &root.join("hlds-delta-fields-build10211.tsv"),
        )
        .unwrap();
        let messages = [crate::signon::UserMessage {
            id: 64,
            size: -1,
            name: "Probe",
        }];
        let clients = [crate::signon::ClientUpdate {
            slot: 0,
            userid: 1,
            userinfo: "\\name\\tester",
            digest: [0; 16],
        }];
        let expected = crate::signon::NewResponse {
            serverinfo: crate::signon::tests::synthetic(&delta.bytes),
            user_messages: &messages,
            full_server_info: "\\hostname\\test",
            allow_shaders: false,
            allow_autoaim: false,
            clients: &clients,
        }
        .encode()
        .unwrap();
        let now = Instant::now();
        let mut sessions = Sessions::new();
        assert!(
            sessions
                .queue_fragments(peer(1), expected.clone(), now)
                .is_err()
        );
        sessions.approve(peer(1), now).unwrap();
        let mut output = sessions
            .queue_fragments(peer(1), expected.clone(), now)
            .unwrap();
        let mut combined = Vec::new();
        let mut index = 1_u16;
        let mut reliable_ack = false;
        loop {
            let seq = u32::from_le_bytes(output[..4].try_into().unwrap());
            assert_eq!(seq & (RELIABLE | FRAGMENT), RELIABLE | FRAGMENT);
            let mut body = output[8..].to_vec();
            munge(&mut body, seq as u8, false);
            let id = u32::from_le_bytes(body[1..5].try_into().unwrap());
            assert_eq!(id >> 16, u32::from(index));
            let count = id & 0xffff;
            let len = usize::from(u16::from_le_bytes(body[7..9].try_into().unwrap()));
            assert_eq!(&body[5..7], &[0, 0]);
            assert_eq!(body[9], 0);
            combined.extend_from_slice(&body[10..10 + len]);
            reliable_ack = !reliable_ack;
            let ack = (seq & SEQUENCE_MASK) | if reliable_ack { RELIABLE } else { 0 };
            let update = sessions
                .receive(peer(1), &packet(u32::from(index), ack, b"\x01"), now)
                .unwrap();
            if u32::from(index) == count {
                assert_eq!(update.outcome, "netchan_fragments_complete");
                break;
            }
            output = update.packet.unwrap();
            index += 1;
        }
        assert!(index > 1);
        assert_eq!(combined, expected);
    }
}
