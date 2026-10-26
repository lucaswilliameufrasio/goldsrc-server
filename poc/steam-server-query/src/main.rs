use std::net::UdpSocket;
use std::time::Duration;

const LISTEN_ADDRESS: &str = "127.0.0.1:27025";
const QUERY_PREFIX: &[u8] = b"\xff\xff\xff\xffTSource Engine Query\0";
const PROTOCOL_VERSION: u8 = 48;
const CHALLENGE: i32 = 0x4353_3136;

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind(LISTEN_ADDRESS)?;
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    println!("Loopback-only CS 1.6 server-query PoC listening on {LISTEN_ADDRESS}");

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
    use super::{CHALLENGE, QUERY_PREFIX, encode_info_response, response_for};

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
}
