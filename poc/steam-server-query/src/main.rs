use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use std::time::Instant;

mod connection;
#[cfg(test)]
mod fragments;
#[cfg(feature = "steam-auth")]
mod netchannel;
#[cfg(test)]
mod serverinfo;
#[cfg(feature = "steam-auth")]
mod steam;
use connection::Connections;
#[cfg(feature = "packet-trace")]
use connection::{CHALLENGE_REQUEST as GAME_CHALLENGE_REQUEST, CONNECT_PREFIX};

#[cfg(feature = "packet-trace")]
use std::fs::OpenOptions;
#[cfg(feature = "packet-trace")]
use std::io::{BufWriter, Write};
#[cfg(all(feature = "packet-trace", unix))]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(feature = "packet-trace")]
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:27025";
const QUERY_PREFIX: &[u8] = b"\xff\xff\xff\xffTSource Engine Query\0";
const PROTOCOL_VERSION: u8 = 48;
const CHALLENGE: i32 = 0x4353_3136;

fn main() -> std::io::Result<()> {
    #[cfg(not(feature = "steam-auth"))]
    if std::env::var_os("GOLDSRC_STEAM_API").is_some() {
        return Err(io::Error::other(
            "GOLDSRC_STEAM_API requires --features steam-auth",
        ));
    }
    #[cfg(not(feature = "packet-trace"))]
    if std::env::var_os("GOLDSRC_PACKET_TRACE").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "GOLDSRC_PACKET_TRACE requires building with --features packet-trace",
        ));
    }

    let listen_address = configured_listen_address()?;
    let socket = UdpSocket::bind(listen_address)?;
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    #[cfg(feature = "packet-trace")]
    let mut packet_trace = PacketTrace::from_environment()?;

    println!("CS 1.6 server-query PoC listening on {listen_address}");
    #[cfg(feature = "packet-trace")]
    if packet_trace.is_some() {
        eprintln!("WARNING: local packet trace enabled; captured datagrams may be sensitive");
    }

    // Hold a full UDP datagram: do not interpret a truncated packet as valid.
    let mut packet = [0_u8; 65535];
    let mut connections = Connections::new();
    #[cfg(feature = "steam-auth")]
    let mut sessions = netchannel::Sessions::new();
    #[cfg(feature = "steam-auth")]
    let mut steam = steam::Steam::from_environment()?;
    #[cfg(feature = "steam-auth")]
    let steam_seconds = if steam.is_some() {
        let seconds = std::env::var("GOLDSRC_STEAM_SECONDS")
            .unwrap_or_else(|_| "120".to_owned())
            .parse::<u64>()
            .map_err(|_| io::Error::other("GOLDSRC_STEAM_SECONDS must be 1..120"))?;
        if !(1..=120).contains(&seconds) {
            return Err(io::Error::other("GOLDSRC_STEAM_SECONDS must be 1..120"));
        }
        seconds
    } else {
        120
    };
    #[cfg(feature = "steam-auth")]
    let steam_deadline = Instant::now() + Duration::from_secs(steam_seconds);
    #[cfg(feature = "steam-auth")]
    if steam.is_some() {
        socket.set_read_timeout(Some(Duration::from_millis(50)))?;
        eprintln!(
            "Steam diagnostic enabled: advertising off, no sign-on, automatic exit in {steam_seconds} seconds"
        );
    }
    loop {
        #[cfg(feature = "steam-auth")]
        if let Some(runtime) = steam.as_mut() {
            if Instant::now() >= steam_deadline {
                eprintln!("Steam diagnostic: session ended; cleaning up runtime");
                return Ok(());
            }
            for event in runtime.poll(Instant::now())? {
                eprintln!("Steam diagnostic: {}", event.outcome);
                #[cfg(feature = "packet-trace")]
                if let Some(trace) = packet_trace.as_mut() {
                    trace.record_outcome(
                        event.peer,
                        event.outcome,
                        None,
                        None,
                        event.result_code,
                    )?;
                }
                let response = if event.outcome == "steam_auth_approved" {
                    match sessions.approve(event.peer, Instant::now()) {
                        Ok(packet) => {
                            #[cfg(feature = "packet-trace")]
                            if let Some(trace) = packet_trace.as_mut() {
                                trace.record_outcome(
                                    event.peer,
                                    "connection_accepted",
                                    None,
                                    None,
                                    None,
                                )?;
                            }
                            packet
                        }
                        Err(_) => {
                            runtime.close_peer(event.peer);
                            b"\xff\xff\xff\xff9Development session capacity reached.\n\0".to_vec()
                        }
                    }
                } else if event.outcome == "steam_auth_revoked" {
                    match sessions.revoke(event.peer, Instant::now()) {
                        Some(packet) => packet,
                        None => continue,
                    }
                } else {
                    let mut packet = b"\xff\xff\xff\xff9".to_vec();
                    packet.extend_from_slice(event.reason.as_bytes());
                    packet.extend_from_slice(b"\n\0");
                    packet
                };
                socket.send_to(&response, event.peer)?;
                #[cfg(feature = "packet-trace")]
                if let Some(trace) = packet_trace.as_mut() {
                    trace.record("out", event.peer, &response)?;
                }
            }
            if let Some(identity) = runtime.server_identity() {
                connections.set_steam_identity(identity);
            }
            for (peer, update) in sessions.tick(Instant::now()) {
                if update.outcome != "netchan_close_probe" {
                    eprintln!("Netchannel diagnostic: {}", update.outcome);
                }
                #[cfg(feature = "packet-trace")]
                if let Some(trace) = packet_trace.as_mut() {
                    trace.record_outcome(peer, update.outcome, None, None, None)?;
                }
                if let Some(packet) = update.packet {
                    socket.send_to(&packet, peer)?;
                }
                if update.finished {
                    runtime.close_peer(peer);
                }
            }
        }
        match socket.recv_from(&mut packet) {
            Ok((packet_length, peer)) => {
                #[cfg(feature = "packet-trace")]
                if let Some(trace) = packet_trace.as_mut() {
                    trace.record("in", peer, &packet[..packet_length])?;
                }

                #[cfg(feature = "steam-auth")]
                if sessions.contains(peer) {
                    if packet[..packet_length].starts_with(connection::CONNECT_PREFIX) {
                        if let Some(response) = sessions.retry_acceptance(peer, Instant::now()) {
                            socket.send_to(&response, peer)?;
                        }
                    } else if !packet[..packet_length].starts_with(&[0xff; 4])
                        && let Some(update) =
                            sessions.receive(peer, &packet[..packet_length], Instant::now())
                    {
                        eprintln!("Netchannel diagnostic: {}", update.outcome);
                        #[cfg(feature = "packet-trace")]
                        if let Some(trace) = packet_trace.as_mut() {
                            trace.record_outcome(peer, update.outcome, None, None, None)?;
                        }
                        if let Some(response) = update.packet {
                            socket.send_to(&response, peer)?;
                        }
                        if update.finished
                            && let Some(runtime) = steam.as_mut()
                        {
                            runtime.close_peer(peer);
                        }
                    }
                    continue;
                }

                #[cfg(feature = "steam-auth")]
                if connection::is_challenge_request(&packet[..packet_length])
                    && steam
                        .as_ref()
                        .is_some_and(|runtime| runtime.server_identity().is_none())
                {
                    continue;
                }

                let response = if let Some(reply) =
                    connections.handle(&packet[..packet_length], peer, Instant::now())?
                {
                    #[cfg(feature = "steam-auth")]
                    let reply = {
                        let mut reply = reply;
                        if let (Some(runtime), Some(ticket)) = (steam.as_mut(), reply.ticket) {
                            reply.outcome = runtime.submit(peer, ticket, Instant::now());
                            eprintln!("Steam diagnostic: {}", reply.outcome);
                            if reply.outcome == "steam_auth_pending" {
                                reply.packet = None;
                            } else {
                                reply.packet = Some(b"\xff\xff\xff\xff9Steam authentication could not be started.\n\0".to_vec());
                            }
                        }
                        reply
                    };
                    #[cfg(feature = "packet-trace")]
                    if let Some(trace) = packet_trace.as_mut() {
                        trace.record_connection(peer, &reply)?;
                    }
                    reply.packet
                } else {
                    response_for(&packet[..packet_length])
                };
                if let Some(response) = response {
                    let sent = socket.send_to(&response, peer)?;
                    if sent != response.len() {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "incomplete UDP response",
                        ));
                    }

                    #[cfg(feature = "packet-trace")]
                    if let Some(trace) = packet_trace.as_mut() {
                        trace.record("out", peer, &response)?;
                    }
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(feature = "packet-trace")]
struct PacketTrace {
    writer: BufWriter<std::fs::File>,
}

#[cfg(feature = "packet-trace")]
impl PacketTrace {
    fn record_connection(&mut self, peer: SocketAddr, reply: &connection::Reply) -> io::Result<()> {
        self.record_outcome(
            peer,
            reply.outcome,
            reply.protocol,
            reply.ticket_length,
            None,
        )
    }

    fn record_outcome(
        &mut self,
        peer: SocketAddr,
        outcome: &str,
        protocol: Option<u32>,
        ticket_length: Option<usize>,
        result_code: Option<i32>,
    ) -> io::Result<()> {
        let mut metadata = String::new();
        if let Some(protocol) = protocol {
            metadata.push_str(&format!(",\"protocol\":{protocol}"));
        }
        if let Some(length) = ticket_length {
            metadata.push_str(&format!(",\"ticket_length\":{length}"));
        }
        if let Some(code) = result_code {
            metadata.push_str(&format!(",\"result_code\":{code}"));
        }
        writeln!(
            self.writer,
            "{{\"record\":\"connection_outcome\",\"peer\":\"{peer}\",\"outcome\":\"{}\"{metadata}}}",
            outcome
        )?;
        self.writer.flush()
    }

    fn from_environment() -> io::Result<Option<Self>> {
        let Some(path) = std::env::var_os("GOLDSRC_PACKET_TRACE") else {
            return Ok(None);
        };

        if path.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "GOLDSRC_PACKET_TRACE must be a non-empty path",
            ));
        }

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(path)?;
        let mut trace = Self {
            writer: BufWriter::new(file),
        };
        writeln!(
            trace.writer,
            "{{\"record\":\"trace_start\",\"format\":\"goldsrc-udp-trace-v1\",\"scope\":\"steam-server-query-poc\"}}"
        )?;
        trace.writer.flush()?;

        Ok(Some(trace))
    }

    fn record(&mut self, direction: &str, peer: SocketAddr, packet: &[u8]) -> io::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        writeln!(
            self.writer,
            "{}",
            format_trace_record(timestamp, direction, peer, packet)
        )?;
        self.writer.flush()
    }
}

#[cfg(feature = "packet-trace")]
fn format_trace_record(
    timestamp: u128,
    direction: &str,
    peer: SocketAddr,
    packet: &[u8],
) -> String {
    // Unknown inbound packets may contain auth tickets or passwords. Fail closed.
    if direction == "in" && !is_safe_trace_request(packet) {
        return format!(
            "{{\"ts_unix_ns\":{timestamp},\"direction\":\"{direction}\",\"peer\":\"{peer}\",\"payload_length\":{},\"payload_redacted\":true,\"kind\":\"{}\"}}",
            packet.len(),
            if packet.starts_with(CONNECT_PREFIX) {
                "connect"
            } else {
                "unknown"
            }
        );
    }

    let mut payload_hex = String::with_capacity(packet.len() * 2);
    for byte in packet {
        payload_hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        payload_hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }

    format!(
        "{{\"ts_unix_ns\":{timestamp},\"direction\":\"{direction}\",\"peer\":\"{peer}\",\"payload_hex\":\"{payload_hex}\"}}"
    )
}

#[cfg(feature = "packet-trace")]
fn is_safe_trace_request(packet: &[u8]) -> bool {
    packet == QUERY_PREFIX
        || (packet.starts_with(QUERY_PREFIX) && packet.len() == QUERY_PREFIX.len() + 4)
        || packet == GAME_CHALLENGE_REQUEST
        || packet.strip_suffix(&[0]) == Some(GAME_CHALLENGE_REQUEST)
}

fn configured_listen_address() -> io::Result<SocketAddr> {
    let listen_address = match std::env::var("LISTEN_ADDRESS") {
        Ok(listen_address) => listen_address,
        Err(std::env::VarError::NotPresent) => DEFAULT_LISTEN_ADDRESS.to_owned(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LISTEN_ADDRESS must be valid text",
            ));
        }
    };

    parse_listen_address(&listen_address)
}

fn parse_listen_address(value: &str) -> io::Result<SocketAddr> {
    let listen_address = value.parse::<SocketAddr>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid LISTEN_ADDRESS: {error}"),
        )
    })?;

    if !is_private_bind_address(listen_address.ip()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LISTEN_ADDRESS must use an explicit loopback, LAN, or Tailscale IP; wildcard and public IPs are rejected",
        ));
    }

    Ok(listen_address)
}

fn is_private_bind_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || is_tailscale_ipv4(address)
        }
        IpAddr::V6(address) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

fn is_tailscale_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn response_for(packet: &[u8]) -> Option<Vec<u8>> {
    if packet == QUERY_PREFIX {
        return Some(encode_challenge());
    }

    let challenge_start = QUERY_PREFIX.len();
    if packet.len() != challenge_start + std::mem::size_of::<i32>()
        || !packet.starts_with(QUERY_PREFIX)
    {
        return None;
    }

    let challenge_bytes: [u8; 4] = packet[challenge_start..].try_into().ok()?;
    let challenge = i32::from_le_bytes(challenge_bytes);
    if challenge != CHALLENGE {
        return None;
    }

    Some(encode_info_response())
}

fn encode_challenge() -> Vec<u8> {
    let mut response = Vec::with_capacity(9);
    response.extend_from_slice(&[0xff; 4]);
    response.push(b'A');
    response.extend_from_slice(&CHALLENGE.to_le_bytes());
    response
}

fn encode_info_response() -> Vec<u8> {
    let mut response = Vec::with_capacity(128);
    response.extend_from_slice(&[0xff; 4]);
    response.push(b'I');
    response.push(PROTOCOL_VERSION);
    push_c_string(&mut response, "CS 1.6 Rust Query PoC");
    push_c_string(&mut response, "de_dust2");
    push_c_string(&mut response, "cstrike");
    push_c_string(&mut response, "Counter-Strike");
    response.extend_from_slice(&10_u16.to_le_bytes());
    response.extend_from_slice(&[0, 32, 0]);
    response.extend_from_slice(b"dl");
    response.extend_from_slice(&[0, 0]);
    push_c_string(&mut response, "0.1.0");
    response
}

fn push_c_string(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(value.as_bytes());
    output.push(0);
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::{
        CHALLENGE, DEFAULT_LISTEN_ADDRESS, QUERY_PREFIX, encode_info_response,
        parse_listen_address, response_for,
    };

    #[test]
    fn should_issue_challenge_for_steam_query() {
        let response = response_for(QUERY_PREFIX).expect("query should receive a challenge");

        assert_eq!(&response[..5], &[0xff, 0xff, 0xff, 0xff, b'A']);
        assert_eq!(&response[5..], &CHALLENGE.to_le_bytes());
    }

    #[test]
    fn should_return_server_info_for_valid_challenge() {
        let mut request = QUERY_PREFIX.to_vec();
        request.extend_from_slice(&CHALLENGE.to_le_bytes());

        let response = response_for(&request).expect("valid challenge should receive info");

        assert_eq!(response, encode_info_response());
        assert_eq!(&response[..6], &[0xff, 0xff, 0xff, 0xff, b'I', 48]);
    }

    #[test]
    fn should_ignore_invalid_or_malformed_queries() {
        let mut request = QUERY_PREFIX.to_vec();
        request.extend_from_slice(&0_i32.to_le_bytes());

        assert!(response_for(&request).is_none());
        assert!(response_for(b"not a Steam query").is_none());
    }

    #[test]
    fn should_ignore_unsupported_or_malformed_game_challenges() {
        for request in [
            b"getchallenge steam\n".as_slice(),
            b"\xff\xff\xff\xffgetchallenge\n".as_slice(),
            b"\xff\xff\xff\xffgetchallenge steam".as_slice(),
            b"\xff\xff\xff\xffgetchallenge steam\nconnect x".as_slice(),
            b"\xff\xff\xff\xffgetchallenge steam\n\0secret".as_slice(),
        ] {
            assert!(response_for(request).is_none());
        }
    }

    #[test]
    fn should_default_to_loopback() {
        let listen_address = parse_listen_address(DEFAULT_LISTEN_ADDRESS)
            .expect("default address should be a valid loopback address");

        assert_eq!(
            listen_address,
            "127.0.0.1:27025".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn should_allow_explicit_lan_and_tailscale_addresses() {
        assert!(parse_listen_address("192.168.1.25:27025").is_ok());
        assert!(parse_listen_address("100.100.1.25:27025").is_ok());
        assert!(parse_listen_address("[fd7a:115c:a1e0::25]:27025").is_ok());
    }

    #[test]
    fn should_reject_wildcard_and_public_addresses() {
        assert!(parse_listen_address("0.0.0.0:27025").is_err());
        assert!(parse_listen_address("[::]:27025").is_err());
        assert!(parse_listen_address("8.8.8.8:27025").is_err());
    }
}

#[cfg(all(test, feature = "packet-trace"))]
mod packet_trace_tests {
    use super::format_trace_record;

    #[test]
    fn should_encode_trace_records_as_json_lines() {
        let peer = "127.0.0.1:27015".parse().unwrap();

        assert_eq!(
            format_trace_record(123, "out", peer, &[0xff, b'A']),
            r#"{"ts_unix_ns":123,"direction":"out","peer":"127.0.0.1:27015","payload_hex":"ff41"}"#
        );
    }

    #[test]
    fn should_redact_connect_and_unknown_inbound_payloads() {
        let peer = "127.0.0.1:27015".parse().unwrap();
        for packet in [
            b"\xff\xff\xff\xffconnect 48 synthetic-ticket".as_slice(),
            b"unknown-password".as_slice(),
        ] {
            let record = format_trace_record(123, "in", peer, packet);
            assert!(record.contains("\"payload_redacted\":true"));
            assert!(!record.contains("payload_hex"));
            assert!(!record.contains("synthetic-ticket"));
        }
    }

    #[test]
    fn should_keep_safe_challenge_and_query_requests() {
        let peer = "127.0.0.1:27015".parse().unwrap();
        for packet in [super::GAME_CHALLENGE_REQUEST, super::QUERY_PREFIX] {
            let record = format_trace_record(123, "in", peer, packet);
            assert!(record.contains("payload_hex"));
            assert!(!record.contains("payload_redacted"));
        }
    }
}
