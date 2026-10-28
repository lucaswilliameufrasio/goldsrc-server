use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:27025";
const QUERY_PREFIX: &[u8] = b"\xff\xff\xff\xffTSource Engine Query\0";
const PROTOCOL_VERSION: u8 = 48;
const CHALLENGE: i32 = 0x4353_3136;

fn main() -> std::io::Result<()> {
    let listen_address = configured_listen_address()?;
    let socket = UdpSocket::bind(listen_address)?;
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    println!("CS 1.6 server-query PoC listening on {listen_address}");

    let mut packet = [0_u8; 1400];
    loop {
        match socket.recv_from(&mut packet) {
            Ok((packet_length, peer)) => {
                if let Some(response) = response_for(&packet[..packet_length]) {
                    socket.send_to(&response, peer)?;
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
