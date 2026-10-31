//! Bounded first-sign-on diagnostic. Not a world/game simulation.
use crate::fragments::{Ack, FragmentQueue};
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
                    b"new" if result != Command::Disconnect => result = Command::New,
                    b"dropclient" | b"disconnect" => result = Command::Disconnect,
                    b"new" => {}
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
    Fragments {
        started: Instant,
        queue: FragmentQueue,
        send_pending: bool,
    },
    Closing {
        started: Instant,
        payload: Vec<u8>,
        last_reliable: Option<u32>,
        reliable_bit: bool,
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

struct Channel {
    acceptance: Vec<u8>,
    accepted: Instant,
    last_send: Instant,
    incoming: u32,
    incoming_reliable: bool,
    outgoing: u32,
    acknowledged: u32,
    reliable_ack: bool,
    outgoing_reliable: bool,
    phase: Phase,
}

impl Channel {
    fn new(peer: SocketAddr, userid: u32, now: Instant) -> Self {
        let mut acceptance = vec![0xff; 4];
        acceptance.extend_from_slice(format!("B {userid} \"{peer}\" 0 10211\0").as_bytes());
        Self {
            acceptance,
            accepted: now,
            last_send: now,
            incoming: 0,
            incoming_reliable: false,
            outgoing: 1,
            acknowledged: 0,
            reliable_ack: false,
            outgoing_reliable: false,
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
            ..
        } = &mut self.phase
        {
            match queue.acknowledge(ack, self.reliable_ack) {
                Ack::Ignored => {}
                Ack::Resend | Ack::Advanced => *send_pending = true,
                Ack::Complete => {
                    self.outgoing_reliable = queue.reliable_bit();
                    self.start_close(now, SIGNON_UNAVAILABLE);
                    return self.transmit(now, "netchan_fragments_complete");
                }
            }
        }
        let mut payload = packet[8..].to_vec();
        munge(&mut payload, sequence as u8, false);
        match command(&payload) {
            Ok(Command::Disconnect) => Update::status("netchan_client_disconnect", true),
            Ok(Command::New) => {
                self.start_close(now, SIGNON_UNAVAILABLE);
                self.transmit(now, "netchan_new")
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
            Phase::AwaitingNew if now.saturating_duration_since(self.accepted) >= WAIT_NEW => {
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
        let userid = self.next_userid;
        self.next_userid = userid.checked_add(1).ok_or("connection_capacity")?;
        let channel = Channel::new(peer, userid, now);
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
        };
        channel
            .transmit(now, "netchan_fragment_start")
            .packet
            .ok_or("netchan_fragment_error")
    }

    pub fn retry_acceptance(&mut self, peer: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        let channel = self.channels.get_mut(&peer)?;
        if matches!(channel.phase, Phase::AwaitingNew)
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
        let mut session = Channel::new(peer(1), 1, now);
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
        let mut session = Channel::new(peer(1), 1, now);
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
        let mut channel = Channel::new(peer(1), 1, now);
        assert_eq!(
            channel
                .receive(&packet(FRAGMENT | 1, 0, b"\x01"), now)
                .outcome,
            "netchan_fragment_unsupported"
        );
        assert!(channel.tick(now + CLOSE_WAIT).unwrap().finished);
        let mut channel = Channel::new(peer(1), 1, now);
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
}
