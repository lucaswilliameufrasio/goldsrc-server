//! Bounded outgoing GoldSrc normal-stream fragment framing. Not wired to sign-on.
use std::io;

const MAX_MESSAGE: usize = 65_536;
const MIN_CHUNK: usize = 256;
const MAX_CHUNK: usize = 1024;

#[derive(Debug, PartialEq)]
pub enum Ack {
    Ignored,
    Resend,
    Advanced,
    Complete,
}

pub struct FragmentQueue {
    message: Vec<u8>,
    chunk_size: usize,
    count: u16,
    index: u16,
    reliable_bit: bool,
    first_sent: Option<u32>,
    last_sent: Option<u32>,
}

impl FragmentQueue {
    pub fn new(message: Vec<u8>, chunk_size: usize, previous_reliable: bool) -> io::Result<Self> {
        if message.is_empty()
            || message.len() > MAX_MESSAGE
            || !(MIN_CHUNK..=MAX_CHUNK).contains(&chunk_size)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid normal fragment buffer or chunk size",
            ));
        }
        let count = message.len().div_ceil(chunk_size) as u16;
        Ok(Self {
            message,
            chunk_size,
            count,
            index: 1,
            reliable_bit: !previous_reliable,
            first_sent: None,
            last_sent: None,
        })
    }

    /// Returns the un-munged fragment metadata followed by the fragment bytes.
    /// The caller sets reliable + fragment bits in the netchannel sequence and
    /// applies Munge2 to the complete payload using that sequence.
    pub fn transmit(&mut self, sequence: u32) -> io::Result<Vec<u8>> {
        if self.index > self.count
            || sequence == 0
            || sequence >= (1 << 30)
            || self.last_sent.is_some_and(|old| sequence <= old)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid fragment sequence or completed queue",
            ));
        }
        let start = (usize::from(self.index) - 1) * self.chunk_size;
        let end = (start + self.chunk_size).min(self.message.len());
        let length = end - start;
        let fragment_id = (u32::from(self.index) << 16) | u32::from(self.count);
        let mut payload = Vec::with_capacity(11 + length);
        payload.push(1); // normal stream present
        payload.extend_from_slice(&fragment_id.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes()); // offset in payload data
        payload.extend_from_slice(&(length as u16).to_le_bytes());
        payload.push(0); // file stream absent
        payload.extend_from_slice(&self.message[start..end]);
        if self.first_sent.is_none() {
            self.first_sent = Some(sequence);
        }
        self.last_sent = Some(sequence);
        Ok(payload)
    }

    pub fn reliable_bit(&self) -> bool {
        self.reliable_bit
    }

    pub fn acknowledge(&mut self, sequence: u32, reliable_bit: bool) -> Ack {
        let Some(sent) = self.last_sent else {
            return Ack::Ignored;
        };
        let Some(first) = self.first_sent else {
            return Ack::Ignored;
        };
        if sequence < first {
            return Ack::Ignored;
        }
        if reliable_bit != self.reliable_bit {
            return if sequence > sent {
                Ack::Resend
            } else {
                Ack::Ignored
            };
        }
        self.index += 1;
        self.last_sent = None;
        self.first_sent = None;
        if self.index > self.count {
            Ack::Complete
        } else {
            self.reliable_bit = !self.reliable_bit;
            Ack::Advanced
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_chunks_and_advances_only_on_matching_ack() {
        let source: Vec<u8> = (0..600).map(|i| i as u8).collect();
        let mut queue = FragmentQueue::new(source.clone(), 256, false).unwrap();
        assert!(queue.reliable_bit());
        let first = queue.transmit(1).unwrap();
        assert_eq!(&first[..10], &[1, 3, 0, 1, 0, 0, 0, 0, 1, 0]);
        assert_eq!(&first[10..], &source[..256]);
        assert_eq!(queue.acknowledge(0, true), Ack::Ignored);
        assert_eq!(queue.acknowledge(1, false), Ack::Ignored);
        assert_eq!(queue.acknowledge(2, false), Ack::Resend);
        assert_eq!(queue.transmit(3).unwrap(), first);
        assert_eq!(queue.acknowledge(3, true), Ack::Advanced);
        assert!(!queue.reliable_bit());
        let second = queue.transmit(4).unwrap();
        assert_eq!(&second[..5], &[1, 3, 0, 2, 0]);
        assert_eq!(&second[10..], &source[256..512]);
        assert_eq!(queue.acknowledge(4, false), Ack::Advanced);
        assert!(queue.reliable_bit());
        let final_fragment = queue.transmit(5).unwrap();
        assert_eq!(&final_fragment[..10], &[1, 3, 0, 3, 0, 0, 0, 88, 0, 0]);
        assert_eq!(&final_fragment[10..], &source[512..]);
        assert_eq!(queue.acknowledge(5, true), Ack::Complete);
        assert!(queue.transmit(6).is_err());
    }

    #[test]
    fn rejects_invalid_bounds_sequences_and_premature_ack() {
        assert!(FragmentQueue::new(vec![], 256, false).is_err());
        assert!(FragmentQueue::new(vec![0; 65_537], 256, false).is_err());
        assert!(FragmentQueue::new(vec![1], 255, false).is_err());
        assert!(FragmentQueue::new(vec![1], 1025, false).is_err());
        let mut queue = FragmentQueue::new(vec![1; 65_536], 256, true).unwrap();
        assert_eq!(queue.count, 256);
        assert!(!queue.reliable_bit());
        assert_eq!(queue.acknowledge(1, false), Ack::Ignored);
        assert!(queue.transmit(0).is_err());
        assert!(queue.transmit(1 << 30).is_err());
        queue.transmit(1).unwrap();
        assert!(queue.transmit(1).is_err());
    }

    #[test]
    fn delayed_ack_for_original_send_is_valid_after_resend() {
        let mut queue = FragmentQueue::new(vec![7; 300], 256, false).unwrap();
        queue.transmit(1).unwrap();
        assert_eq!(queue.acknowledge(2, false), Ack::Resend);
        queue.transmit(3).unwrap();
        assert_eq!(queue.acknowledge(1, true), Ack::Advanced);
        assert!(!queue.reliable_bit());
    }
}
