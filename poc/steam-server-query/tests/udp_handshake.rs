use std::io;
use std::net::UdpSocket;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Server(Child);

#[cfg(feature = "packet-trace")]
struct TraceFile(std::path::PathBuf);

#[cfg(feature = "packet-trace")]
impl Drop for TraceFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn should_serve_query_and_game_challenge_over_udp() {
    let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut command = Command::new(env!("CARGO_BIN_EXE_steam-server-query-poc"));
    command
        .env("LISTEN_ADDRESS", address.to_string())
        .env_remove("GOLDSRC_PACKET_TRACE")
        .env_remove("GOLDSRC_STEAM_API")
        .env_remove("GOLDSRC_DELTA_LIST")
        .env_remove("GOLDSRC_DELTA_CATALOG")
        .stdout(Stdio::null());
    #[cfg(feature = "packet-trace")]
    let trace = TraceFile(std::env::temp_dir().join(format!(
        "goldsrc-handshake-test-{}-{}.jsonl",
        std::process::id(),
        address.port()
    )));
    #[cfg(feature = "packet-trace")]
    command.env("GOLDSRC_PACKET_TRACE", &trace.0);
    let mut server = Server(command.spawn().unwrap());
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let request = b"\xff\xff\xff\xffgetchallenge steam\n";
    let mut packet = [0_u8; 1400];
    let mut received = None;
    for _ in 0..40 {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited early"
        );
        client.send_to(request, address).unwrap();
        match client.recv_from(&mut packet) {
            Ok((length, peer)) => {
                assert_eq!(peer, address);
                received = Some(packet[..length].to_vec());
                break;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("UDP receive failed: {error}"),
        }
    }
    let challenge_reply = received.expect("game challenge response");
    assert!(challenge_reply.starts_with(b"\xff\xff\xff\xffA00000000 "));
    assert!(challenge_reply.ends_with(b" 3 0 0\n\0"));
    let challenge_text = std::str::from_utf8(&challenge_reply[4..]).unwrap();
    let challenge: u32 = challenge_text
        .split_ascii_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=i32::MAX as u32).contains(&challenge));
    let mut connect = vec![0xff; 4];
    connect.extend_from_slice(
        format!(
            "connect 48 {challenge} \"\\prot\\3\" \"\\name\\Test\\password\\synthetic-password\"\n"
        )
        .as_bytes(),
    );
    connect.extend_from_slice(b"synthetic-ticket");

    // A different source port cannot use the first client's challenge.
    let intruder = UdpSocket::bind("127.0.0.1:0").unwrap();
    intruder
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    intruder.send_to(&connect, address).unwrap();
    let (length, _) = intruder.recv_from(&mut packet).unwrap();
    assert_eq!(
        &packet[..length],
        b"\xff\xff\xff\xff9Invalid or expired connection challenge.\n\0"
    );

    client.send_to(&connect, address).unwrap();
    let (length, _) = client.recv_from(&mut packet).unwrap();
    assert!(packet[..length].starts_with(b"\xff\xff\xff\xff9Development PoC:"));
    assert_eq!(packet[length - 1], 0);

    // The well-formed envelope consumed the token, even though auth is unavailable.
    client.send_to(&connect, address).unwrap();
    let (length, _) = client.recv_from(&mut packet).unwrap();
    assert_eq!(
        &packet[..length],
        b"\xff\xff\xff\xff9Invalid or expired connection challenge.\n\0"
    );

    let query = b"\xff\xff\xff\xffTSource Engine Query\0";
    client.send_to(query, address).unwrap();
    let (length, _) = client.recv_from(&mut packet).unwrap();
    assert_eq!(length, 9);
    assert_eq!(&packet[..5], b"\xff\xff\xff\xffA");
    let mut info_request = query.to_vec();
    info_request.extend_from_slice(&packet[5..9]);
    client.send_to(&info_request, address).unwrap();
    let (length, _) = client.recv_from(&mut packet).unwrap();
    assert_eq!(length, 75);
    assert_eq!(&packet[..6], b"\xff\xff\xff\xffI0");

    #[cfg(feature = "packet-trace")]
    {
        let mut contents = String::new();
        for _ in 0..40 {
            contents = std::fs::read_to_string(&trace.0).unwrap();
            if contents.lines().count() >= 17 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(contents.lines().count(), 17);
        assert!(contents.contains("\"kind\":\"connect\""));
        assert!(contents.contains("\"payload_redacted\":true"));
        assert!(!contents.contains("73796e7468657469632d7469636b6574"));
        assert!(!contents.contains("73796e7468657469632d70617373776f7264"));
        assert!(contents.contains(
            "\"outcome\":\"steam_auth_unavailable\",\"protocol\":48,\"ticket_length\":16"
        ));
        assert_eq!(
            contents
                .matches("\"outcome\":\"invalid_challenge\"")
                .count(),
            2
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::metadata(&trace.0).unwrap().permissions().mode();
            assert_eq!(permissions & 0o777, 0o600);
        }
    }
}

#[test]
fn delta_configuration_must_be_complete_before_startup() {
    let output = Command::new(env!("CARGO_BIN_EXE_steam-server-query-poc"))
        .env("GOLDSRC_DELTA_LIST", "unused")
        .env_remove("GOLDSRC_DELTA_CATALOG")
        .env_remove("GOLDSRC_STEAM_API")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be supplied together"));
}
