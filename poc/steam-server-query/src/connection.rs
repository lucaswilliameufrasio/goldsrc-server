use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub const CHALLENGE_REQUEST: &[u8] = b"\xff\xff\xff\xffgetchallenge steam\n";
pub const CONNECT_PREFIX: &[u8] = b"\xff\xff\xff\xffconnect ";
const TTL: Duration = Duration::from_secs(30);
const CAPACITY: usize = 1024;

pub struct Reply<'a> {
    pub packet: Option<Vec<u8>>,
    #[cfg_attr(not(feature = "packet-trace"), allow(dead_code))]
    pub outcome: &'static str,
    pub protocol: Option<u32>,
    pub ticket_length: Option<usize>,
    #[cfg_attr(not(feature = "steam-auth"), allow(dead_code))]
    pub ticket: Option<&'a [u8]>,
}

impl Reply<'_> {
    fn rejection(outcome: &'static str, reason: &str) -> Self {
        let mut packet = b"\xff\xff\xff\xff9".to_vec();
        packet.extend_from_slice(reason.as_bytes());
        packet.extend_from_slice(b"\n\0");
        Self {
            packet: Some(packet),
            outcome,
            protocol: None,
            ticket_length: None,
            ticket: None,
        }
    }
}

struct Challenge {
    value: u32,
    issued: Instant,
}

pub struct Connections {
    pending: HashMap<SocketAddr, Challenge>,
    steam_identity: u64,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            steam_identity: 0,
        }
    }

    #[cfg(feature = "steam-auth")]
    pub fn set_steam_identity(&mut self, identity: u64) {
        self.steam_identity = identity;
    }

    pub fn handle<'a>(
        &mut self,
        packet: &'a [u8],
        peer: SocketAddr,
        now: Instant,
    ) -> io::Result<Option<Reply<'a>>> {
        self.handle_with_random(packet, peer, now, || {
            getrandom::u32().map_err(|_| io::Error::other("OS challenge entropy unavailable"))
        })
    }

    fn handle_with_random<'a>(
        &mut self,
        packet: &'a [u8],
        peer: SocketAddr,
        now: Instant,
        random: impl FnOnce() -> io::Result<u32>,
    ) -> io::Result<Option<Reply<'a>>> {
        if !is_challenge_request(packet) && !packet.starts_with(CONNECT_PREFIX) {
            return Ok(None);
        }
        self.pending
            .retain(|_, challenge| now.saturating_duration_since(challenge.issued) < TTL);

        if is_challenge_request(packet) {
            let value = if let Some(challenge) = self.pending.get(&peer) {
                challenge.value
            } else {
                if self.pending.len() >= CAPACITY {
                    return Ok(Some(Reply {
                        packet: None,
                        outcome: "challenge_capacity",
                        protocol: None,
                        ticket_length: None,
                        ticket: None,
                    }));
                }
                // Positive signed range for GoldSrc's decimal integer parser.
                let value = (random()? & 0x7fff_ffff).max(1);
                self.pending.insert(peer, Challenge { value, issued: now });
                value
            };
            let mut response = vec![0xff; 4];
            response.extend_from_slice(
                format!("A00000000 {value} 3 {} 0\n\0", self.steam_identity).as_bytes(),
            );
            return Ok(Some(Reply {
                packet: Some(response),
                outcome: "challenge_issued",
                protocol: None,
                ticket_length: None,
                ticket: None,
            }));
        }

        let parsed = match parse_connect(packet) {
            Ok(parsed) => parsed,
            Err(reason) => return Ok(Some(Reply::rejection(reason.code(), reason.message()))),
        };
        let mut reply = if self.pending.get(&peer).map(|entry| entry.value)
            != Some(parsed.challenge)
        {
            Reply::rejection(
                "invalid_challenge",
                "Invalid or expired connection challenge.",
            )
        } else {
            self.pending.remove(&peer);
            Reply::rejection(
                "steam_auth_unavailable",
                "Development PoC: connect envelope validated; Steam authentication and sign-on are not implemented.",
            )
        };
        reply.protocol = Some(parsed.protocol);
        reply.ticket_length = Some(parsed.ticket_length);
        if reply.outcome == "steam_auth_unavailable" {
            reply.ticket = Some(parsed.ticket);
        }
        Ok(Some(reply))
    }
}

pub fn is_challenge_request(packet: &[u8]) -> bool {
    packet == CHALLENGE_REQUEST || packet.strip_suffix(&[0]) == Some(CHALLENGE_REQUEST)
}

// Only safe numeric metadata leaves this parser. Never derive Debug for raw tickets/info.
struct Connect<'a> {
    protocol: u32,
    challenge: u32,
    ticket_length: usize,
    ticket: &'a [u8],
}

#[derive(Debug, PartialEq)]
enum ConnectError {
    Malformed,
    Protocol,
    Auth,
    Ticket,
}

impl ConnectError {
    fn code(&self) -> &'static str {
        match self {
            Self::Malformed => "malformed_connect",
            Self::Protocol => "unsupported_protocol",
            Self::Auth => "unsupported_auth",
            Self::Ticket => "invalid_ticket_length",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::Malformed => "Malformed connection envelope.",
            Self::Protocol => "Only GoldSrc protocol 48 is supported.",
            Self::Auth => "Only official Steam authentication type 3 is supported.",
            Self::Ticket => "Missing or oversized Steam authentication ticket.",
        }
    }
}

fn parse_connect(packet: &[u8]) -> Result<Connect<'_>, ConnectError> {
    let payload = packet
        .strip_prefix(CONNECT_PREFIX)
        .ok_or(ConnectError::Malformed)?;
    let end = payload
        .iter()
        .position(|byte| matches!(byte, b'\n' | 0))
        .ok_or(ConnectError::Malformed)?;
    if end > 2048 {
        return Err(ConnectError::Malformed);
    }
    let mut line = &payload[..end];
    if line.iter().any(u8::is_ascii_control) {
        return Err(ConnectError::Malformed);
    }
    let protocol = decimal(take_word(&mut line)?)?;
    if protocol != 48 {
        return Err(ConnectError::Protocol);
    }
    let challenge = decimal(take_word(&mut line)?)?;
    let protinfo = take_quoted(&mut line, 1023)?;
    let userinfo = take_quoted(&mut line, 255)?;
    if !line.iter().all(|byte| *byte == b' ') {
        return Err(ConnectError::Malformed);
    }
    let auth = validate_info(protinfo, true)?;
    validate_info(userinfo, false)?;
    if auth != Some(b"3".as_slice()) {
        return Err(ConnectError::Auth);
    }
    let ticket_length = payload.len() - end - 1;
    if !(1..1024).contains(&ticket_length) {
        return Err(ConnectError::Ticket);
    }
    Ok(Connect {
        protocol,
        challenge,
        ticket_length,
        ticket: &payload[end + 1..],
    })
}

fn take_word<'a>(line: &mut &'a [u8]) -> Result<&'a [u8], ConnectError> {
    let end = line
        .iter()
        .position(|byte| *byte == b' ')
        .ok_or(ConnectError::Malformed)?;
    let word = &line[..end];
    *line = line[end..].trim_ascii_start();
    Ok(word)
}

fn decimal(word: &[u8]) -> Result<u32, ConnectError> {
    if word.is_empty() || word.len() > 10 || !word.iter().all(u8::is_ascii_digit) {
        return Err(ConnectError::Malformed);
    }
    let mut value = 0_u32;
    for byte in word {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u32::from(byte - b'0')))
            .ok_or(ConnectError::Malformed)?;
    }
    Ok(value)
}

fn take_quoted<'a>(line: &mut &'a [u8], max: usize) -> Result<&'a [u8], ConnectError> {
    let content = line.strip_prefix(b"\"").ok_or(ConnectError::Malformed)?;
    let end = content
        .iter()
        .position(|byte| *byte == b'"')
        .ok_or(ConnectError::Malformed)?;
    if end > max {
        return Err(ConnectError::Malformed);
    }
    let value = &content[..end];
    let rest = &content[end + 1..];
    if !rest.is_empty() && rest[0] != b' ' {
        return Err(ConnectError::Malformed);
    }
    *line = rest.trim_ascii_start();
    Ok(value)
}

fn validate_info(info: &[u8], find_auth: bool) -> Result<Option<&[u8]>, ConnectError> {
    if info
        .iter()
        .any(|byte| byte.is_ascii_control() || matches!(byte, b'"' | b';'))
    {
        return Err(ConnectError::Malformed);
    }
    if info.is_empty() {
        return Ok(None);
    }
    let mut fields = info
        .strip_prefix(b"\\")
        .ok_or(ConnectError::Malformed)?
        .split(|byte| *byte == b'\\');
    let mut keys: Vec<&[u8]> = Vec::new();
    let mut auth = None;
    while let Some(key) = fields.next() {
        let value = fields.next().ok_or(ConnectError::Malformed)?;
        if key.is_empty() || keys.contains(&key) {
            return Err(ConnectError::Malformed);
        }
        keys.push(key);
        if find_auth && key == b"prot" {
            auth = Some(value);
        }
    }
    Ok(auth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn connect(challenge: u32) -> Vec<u8> {
        let mut packet = vec![0xff; 4];
        packet.extend_from_slice(format!("connect 48 {challenge} \"\\prot\\3\\unique\\-1\" \"\\name\\Test\\password\\synthetic\"\n").as_bytes());
        packet.extend_from_slice(b"synthetic-ticket\0\xff\n");
        packet
    }

    fn issue(server: &mut Connections, port: u16, now: Instant, value: u32) -> Reply<'static> {
        server
            .handle_with_random(CHALLENGE_REQUEST, peer(port), now, || Ok(value))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn challenges_are_peer_bound_consumed_and_replay_rejected() {
        let now = Instant::now();
        let mut server = Connections::new();
        let issued = issue(&mut server, 2000, now, 123);
        assert_eq!(
            issued.packet.unwrap(),
            b"\xff\xff\xff\xffA00000000 123 3 0 0\n\0"
        );
        let packet = connect(123);
        let wrong_peer = server.handle(&packet, peer(2001), now).unwrap().unwrap();
        assert_eq!(wrong_peer.outcome, "invalid_challenge");
        let valid = server.handle(&packet, peer(2000), now).unwrap().unwrap();
        assert_eq!(valid.outcome, "steam_auth_unavailable");
        assert_eq!(valid.protocol, Some(48));
        assert_eq!(valid.ticket_length, Some(19));
        assert_eq!(valid.ticket, Some(b"synthetic-ticket\0\xff\n".as_slice()));
        assert!(
            valid
                .packet
                .unwrap()
                .starts_with(b"\xff\xff\xff\xff9Development PoC:")
        );
        assert_eq!(
            server
                .handle(&packet, peer(2000), now)
                .unwrap()
                .unwrap()
                .outcome,
            "invalid_challenge"
        );
    }

    #[test]
    fn wrong_token_does_not_consume_correct_token() {
        let now = Instant::now();
        let mut server = Connections::new();
        issue(&mut server, 2000, now, 123);
        assert_eq!(
            server
                .handle(&connect(124), peer(2000), now)
                .unwrap()
                .unwrap()
                .outcome,
            "invalid_challenge"
        );
        assert_eq!(
            server
                .handle(&connect(123), peer(2000), now)
                .unwrap()
                .unwrap()
                .outcome,
            "steam_auth_unavailable"
        );
    }

    #[test]
    fn retries_reuse_token_without_extending_expiry() {
        let now = Instant::now();
        let mut server = Connections::new();
        let first = issue(&mut server, 2000, now, 123);
        let retry = issue(&mut server, 2000, now + TTL - Duration::from_secs(1), 456);
        assert_eq!(first.packet, retry.packet);
        assert_eq!(
            server
                .handle(&connect(123), peer(2000), now + TTL)
                .unwrap()
                .unwrap()
                .outcome,
            "invalid_challenge"
        );
        issue(&mut server, 2000, now + TTL, 456);
        assert_eq!(
            server
                .handle(&connect(456), peer(2000), now + TTL)
                .unwrap()
                .unwrap()
                .outcome,
            "steam_auth_unavailable"
        );
    }

    #[test]
    fn capacity_is_bounded_and_recovers_after_expiry() {
        let now = Instant::now();
        let mut server = Connections::new();
        for port in 1..=CAPACITY as u16 {
            issue(&mut server, port, now, u32::from(port));
        }
        let busy = issue(&mut server, 2000, now, 9999);
        assert_eq!(busy.outcome, "challenge_capacity");
        assert!(busy.packet.is_none());
        assert_eq!(server.pending.len(), CAPACITY);
        assert_eq!(issue(&mut server, 1, now, 9999).outcome, "challenge_issued");
        assert_eq!(
            issue(&mut server, 2000, now + TTL, 9999).outcome,
            "challenge_issued"
        );
        assert_eq!(server.pending.len(), 1);
    }

    #[test]
    fn entropy_failure_creates_no_challenge() {
        let mut server = Connections::new();
        let result =
            server.handle_with_random(CHALLENGE_REQUEST, peer(2000), Instant::now(), || {
                Err(io::Error::other("test entropy error"))
            });
        assert!(result.is_err());
        assert!(server.pending.is_empty());
    }

    #[test]
    fn unsupported_requests_allocate_no_state() {
        let mut server = Connections::new();
        for request in [
            b"getchallenge steam\n".as_slice(),
            b"\xff\xff\xff\xffgetchallenge\n".as_slice(),
            b"\xff\xff\xff\xffgetchallenge steam".as_slice(),
            b"\xff\xff\xff\xffgetchallenge steam\n\0extra".as_slice(),
        ] {
            assert!(
                server
                    .handle_with_random(request, peer(2000), Instant::now(), || panic!(
                        "must not request entropy"
                    ))
                    .unwrap()
                    .is_none()
            );
        }
        assert!(server.pending.is_empty());
    }

    #[test]
    fn challenges_cannot_be_used_from_another_ip() {
        let now = Instant::now();
        let mut server = Connections::new();
        issue(&mut server, 2000, now, 123);
        let other = SocketAddr::from(([127, 0, 0, 2], 2000));
        assert_eq!(
            server
                .handle(&connect(123), other, now)
                .unwrap()
                .unwrap()
                .outcome,
            "invalid_challenge"
        );
        let ipv6: SocketAddr = "[::1]:2000".parse().unwrap();
        assert_eq!(
            server
                .handle(&connect(123), ipv6, now)
                .unwrap()
                .unwrap()
                .outcome,
            "invalid_challenge"
        );
    }

    #[test]
    fn supports_nul_terminated_challenge_and_connect_line() {
        let now = Instant::now();
        let mut server = Connections::new();
        let request = [CHALLENGE_REQUEST, &[0]].concat();
        assert!(
            server
                .handle_with_random(&request, peer(2000), now, || Ok(123))
                .unwrap()
                .is_some()
        );
        let mut packet = connect(123);
        let separator = packet.iter().position(|byte| *byte == b'\n').unwrap();
        packet[separator] = 0;
        assert_eq!(
            server
                .handle(&packet, peer(2000), now)
                .unwrap()
                .unwrap()
                .outcome,
            "steam_auth_unavailable"
        );
    }

    #[test]
    fn rejects_malformed_protocol_auth_and_ticket_without_consuming_challenge() {
        let now = Instant::now();
        let mut server = Connections::new();
        issue(&mut server, 2000, now, 123);
        for (body, expected) in [
            (
                "47 123 \"\\prot\\3\" \"\\name\\Test\"\nticket",
                "unsupported_protocol",
            ),
            (
                "48 123 \"\\prot\\2\" \"\\name\\Test\"\nticket",
                "unsupported_auth",
            ),
            (
                "48 123 \"\\prot\\3\" \"\\name\\Test\"\n",
                "invalid_ticket_length",
            ),
            (
                "48 123 \"\\prot\\3\\prot\\3\" \"\"\nticket",
                "malformed_connect",
            ),
            (
                "48 123 \"\\prot\\3\" \"\\name\"\nticket",
                "malformed_connect",
            ),
            (
                "48 123 \"\\prot\\3\" \"\\name\\Test\" extra\nticket",
                "malformed_connect",
            ),
            (
                "48 4294967296 \"\\prot\\3\" \"\"\nticket",
                "malformed_connect",
            ),
            ("48 -1 \"\\prot\\3\" \"\"\nticket", "malformed_connect"),
            ("48 123 \"\\prot\\3\"\"\"\nticket", "malformed_connect"),
            ("48 123 \"\\prot\\3\" \"\"", "malformed_connect"),
        ] {
            let packet = [CONNECT_PREFIX, body.as_bytes()].concat();
            assert_eq!(
                server
                    .handle(&packet, peer(2000), now)
                    .unwrap()
                    .unwrap()
                    .outcome,
                expected
            );
            assert_eq!(server.pending.len(), 1);
        }
        assert_eq!(
            server
                .handle(&connect(123), peer(2000), now)
                .unwrap()
                .unwrap()
                .outcome,
            "steam_auth_unavailable"
        );
    }

    #[test]
    fn bounded_parser_handles_binary_and_truncation_without_panicking() {
        let packet = connect(123);
        for length in 0..packet.len() {
            let _ = parse_connect(&packet[..length]);
        }
        for length in [1024, 65500] {
            let oversized = [
                b"\xff\xff\xff\xffconnect 48 123 \"\\prot\\3\" \"\"\n".as_slice(),
                &vec![1; length],
            ]
            .concat();
            assert_eq!(parse_connect(&oversized).err(), Some(ConnectError::Ticket));
        }
        let oversized_info = [
            b"\xff\xff\xff\xffconnect 48 123 \"\\prot\\3\" \"\\name\\".as_slice(),
            &vec![b'x'; 300],
            b"\"\nticket",
        ]
        .concat();
        assert_eq!(
            parse_connect(&oversized_info).err(),
            Some(ConnectError::Malformed)
        );
        let oversized_line = [CONNECT_PREFIX, &vec![b'1'; 2049], b"\nticket"].concat();
        assert_eq!(
            parse_connect(&oversized_line).err(),
            Some(ConnectError::Malformed)
        );
    }
}
