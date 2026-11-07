//! Bounded, source-bound A2S_INFO challenges for the diagnostic query endpoint.
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub const QUERY_PREFIX: &[u8] = b"\xff\xff\xff\xffTSource Engine Query\0";
const CHALLENGE_TTL: Duration = Duration::from_secs(30);
const CHALLENGE_CAPACITY: usize = 1024;

struct Challenge {
    value: u32,
    issued: Instant,
}

/// Volatile A2S challenge state. This bounds memory but does not rate-limit peers.
pub struct QueryChallenges {
    pending: HashMap<SocketAddr, Challenge>,
}

impl QueryChallenges {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    pub fn handle(
        &mut self,
        packet: &[u8],
        peer: SocketAddr,
        now: Instant,
        info_response: &[u8],
    ) -> io::Result<Option<Vec<u8>>> {
        self.handle_with_random(packet, peer, now, info_response, || {
            getrandom::u32().map_err(|_| io::Error::other("A2S challenge entropy unavailable"))
        })
    }

    fn handle_with_random(
        &mut self,
        packet: &[u8],
        peer: SocketAddr,
        now: Instant,
        info_response: &[u8],
        random: impl FnOnce() -> io::Result<u32>,
    ) -> io::Result<Option<Vec<u8>>> {
        if packet == QUERY_PREFIX {
            self.expire(now);
            let value = if let Some(challenge) = self.pending.get(&peer) {
                challenge.value
            } else {
                if self.pending.len() >= CHALLENGE_CAPACITY {
                    return Ok(None);
                }
                let value = random()?;
                self.pending.insert(peer, Challenge { value, issued: now });
                value
            };
            return Ok(Some(encode_challenge(value)));
        }

        let Some(challenge_bytes) = packet.strip_prefix(QUERY_PREFIX) else {
            return Ok(None);
        };
        if challenge_bytes.len() != std::mem::size_of::<u32>() {
            return Ok(None);
        }

        self.expire(now);
        let value = u32::from_le_bytes(
            challenge_bytes
                .try_into()
                .map_err(|_| io::Error::other("invalid A2S challenge width"))?,
        );
        let Some(challenge) = self.pending.get(&peer) else {
            return Ok(None);
        };
        if challenge.value != value {
            return Ok(None);
        }

        Ok(Some(info_response.to_vec()))
    }

    fn expire(&mut self, now: Instant) {
        self.pending
            .retain(|_, challenge| now.saturating_duration_since(challenge.issued) < CHALLENGE_TTL);
    }
}

fn encode_challenge(value: u32) -> Vec<u8> {
    let mut response = Vec::with_capacity(9);
    response.extend_from_slice(&[0xff; 4]);
    response.push(b'A');
    response.extend_from_slice(&value.to_le_bytes());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn challenge_packet(value: u32) -> Vec<u8> {
        let mut packet = QUERY_PREFIX.to_vec();
        packet.extend_from_slice(&value.to_le_bytes());
        packet
    }

    fn issued_value(response: &[u8]) -> u32 {
        assert_eq!(response.len(), 9);
        assert_eq!(&response[..5], b"\xff\xff\xff\xffA");
        u32::from_le_bytes(response[5..9].try_into().unwrap())
    }

    #[test]
    fn challenge_is_random_peer_bound_and_reusable_until_expiry() {
        let now = Instant::now();
        let mut challenges = QueryChallenges::new();
        let expected = 0xdead_beef;
        let response = challenges
            .handle_with_random(QUERY_PREFIX, peer(27015), now, b"info", || Ok(expected))
            .unwrap()
            .unwrap();
        assert_eq!(issued_value(&response), expected);

        let request = challenge_packet(expected);
        assert!(
            challenges
                .handle_with_random(&request, peer(27016), now, b"info", || {
                    panic!("info lookup must not request entropy")
                })
                .unwrap()
                .is_none()
        );
        let wrong_token = challenge_packet(expected ^ 1);
        assert!(
            challenges
                .handle_with_random(&wrong_token, peer(27015), now, b"info", || {
                    panic!("wrong token must not request entropy")
                })
                .unwrap()
                .is_none()
        );
        assert_eq!(
            challenges
                .handle_with_random(&request, peer(27015), now, b"info", || {
                    panic!("valid challenge must not request entropy")
                })
                .unwrap(),
            Some(b"info".to_vec())
        );
        assert_eq!(
            challenges
                .handle_with_random(&request, peer(27015), now, b"info-again", || {
                    panic!("reusable challenge must not request entropy")
                })
                .unwrap(),
            Some(b"info-again".to_vec())
        );
    }

    #[test]
    fn retries_reuse_challenge_without_extending_expiry() {
        let now = Instant::now();
        let mut challenges = QueryChallenges::new();
        let first = challenges
            .handle_with_random(QUERY_PREFIX, peer(27015), now, b"info", || Ok(11))
            .unwrap()
            .unwrap();
        let retry = challenges
            .handle_with_random(
                QUERY_PREFIX,
                peer(27015),
                now + CHALLENGE_TTL - Duration::from_millis(1),
                b"info",
                || panic!("live peer should reuse the challenge"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(retry, first);

        let renewed = challenges
            .handle_with_random(
                QUERY_PREFIX,
                peer(27015),
                now + CHALLENGE_TTL,
                b"info",
                || Ok(12),
            )
            .unwrap()
            .unwrap();
        assert_eq!(issued_value(&renewed), 12);
        let stale_request = challenge_packet(11);
        assert!(
            challenges
                .handle_with_random(
                    &stale_request,
                    peer(27015),
                    now + CHALLENGE_TTL,
                    b"info",
                    || panic!("stale response must not request entropy"),
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn capacity_is_bounded_and_recovers_after_expiry() {
        let now = Instant::now();
        let mut challenges = QueryChallenges::new();
        for index in 0..CHALLENGE_CAPACITY {
            let port = u16::try_from(1000 + index).unwrap();
            assert!(
                challenges
                    .handle_with_random(QUERY_PREFIX, peer(port), now, b"info", || {
                        Ok(u32::from(port))
                    })
                    .unwrap()
                    .is_some()
            );
        }

        assert!(
            challenges
                .handle_with_random(QUERY_PREFIX, peer(5000), now, b"info", || {
                    panic!("full challenge table must drop before requesting entropy")
                })
                .unwrap()
                .is_none()
        );
        assert!(
            challenges
                .handle_with_random(
                    QUERY_PREFIX,
                    peer(5000),
                    now + CHALLENGE_TTL,
                    b"info",
                    || Ok(5000),
                )
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn malformed_queries_and_entropy_failure_allocate_no_state() {
        let now = Instant::now();
        let mut challenges = QueryChallenges::new();
        for packet in [
            b"not a query".as_slice(),
            &QUERY_PREFIX[..QUERY_PREFIX.len() - 1],
            &[QUERY_PREFIX, &[1, 2, 3]].concat(),
        ] {
            assert!(
                challenges
                    .handle_with_random(packet, peer(27015), now, b"info", || {
                        panic!("unsupported packet must not request entropy")
                    })
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            challenges
                .handle_with_random(&challenge_packet(1), peer(27015), now, b"info", || {
                    panic!("response without issued token must not request entropy")
                })
                .unwrap()
                .is_none()
        );
        assert!(
            challenges
                .handle_with_random(QUERY_PREFIX, peer(27015), now, b"info", || {
                    Err(io::Error::other("synthetic entropy failure"))
                })
                .is_err()
        );
        assert!(challenges.pending.is_empty());
        assert!(
            challenges
                .handle_with_random(QUERY_PREFIX, peer(27015), now, b"info", || Ok(2))
                .unwrap()
                .is_some()
        );
    }
}
