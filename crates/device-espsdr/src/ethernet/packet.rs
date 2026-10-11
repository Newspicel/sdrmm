use sdrmm_device::{BlockGap, DeviceError};

pub(crate) const PACKET_BYTES: usize = 1376;
pub(crate) const HEADER_BYTES: usize = 32;
pub(crate) const PAYLOAD_BYTES: usize = PACKET_BYTES - HEADER_BYTES;
const PACKET_SAMPLES: u64 = (PAYLOAD_BYTES / 2) as u64;
const MAGIC: &[u8; 4] = b"ESR2";
const VERSION: u16 = 2;
const BITS: u16 = 8;
const MICROS: u64 = 1_000_000;
pub(crate) const RATES: [u32; 5] = [4_000_000, 8_000_000, 16_000_000, 20_000_000, 40_000_000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) epoch: u32,
    pub(crate) rate: u32,
    pub(crate) sample: u64,
    pub(crate) time_us: u64,
}

fn field<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[at..at + N]);
    out
}

impl Header {
    pub(crate) fn parse(datagram: &[u8]) -> Result<Self, DeviceError> {
        let bad = |why: String| DeviceError::Io(format!("ESP-SDR packet: {why}"));
        if datagram.len() != PACKET_BYTES {
            return Err(bad(format!(
                "{} bytes, expected {PACKET_BYTES}",
                datagram.len()
            )));
        }
        if &datagram[..4] != MAGIC {
            return Err(bad("no ESR2 magic".to_string()));
        }
        let version = u16::from_le_bytes(field(datagram, 4));
        let bits = u16::from_le_bytes(field(datagram, 6));
        if version != VERSION || bits != BITS {
            return Err(bad(format!("version {version} with {bits} bits")));
        }
        let header = Self {
            epoch: u32::from_le_bytes(field(datagram, 8)),
            rate: u32::from_le_bytes(field(datagram, 12)),
            sample: u64::from_le_bytes(field(datagram, 16)),
            time_us: u64::from_le_bytes(field(datagram, 24)),
        };
        if !RATES.contains(&header.rate) {
            return Err(bad(format!("rate {}", header.rate)));
        }
        Ok(header)
    }

    fn end_us(&self) -> u64 {
        self.time_us + PACKET_SAMPLES * MICROS / u64::from(self.rate)
    }

    #[cfg(test)]
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PACKET_BYTES);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&BITS.to_le_bytes());
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&self.sample.to_le_bytes());
        out.extend_from_slice(&self.time_us.to_le_bytes());
        out.resize(PACKET_BYTES, 0);
        out
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    Next,
    After(BlockGap),
    Stale,
}

#[derive(Debug, Default)]
pub(crate) struct Tracker {
    last: Option<Header>,
}

impl Tracker {
    pub(crate) fn place(&mut self, header: Header) -> Placed {
        let Some(last) = self.last else {
            self.last = Some(header);
            return Placed::Next;
        };
        let gap = if header.epoch == last.epoch {
            let expected = last.sample + PACKET_SAMPLES;
            if header.sample < expected {
                return Placed::Stale;
            }
            BlockGap {
                exact: header.sample - expected,
                estimated: 0,
            }
        } else if header.epoch < last.epoch {
            return Placed::Stale;
        } else {
            let quiet_us = header.time_us.saturating_sub(last.end_us());
            BlockGap {
                exact: 0,
                estimated: quiet_us * u64::from(header.rate) / MICROS,
            }
        };
        self.last = Some(header);
        if gap == BlockGap::default() {
            Placed::Next
        } else {
            Placed::After(gap)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000_000;

    fn header(epoch: u32, sample: u64, time_us: u64) -> Header {
        Header {
            epoch,
            rate: RATE,
            sample,
            time_us,
        }
    }

    #[test]
    fn a_packet_round_trips_and_foreign_ones_are_refused() {
        let sent = header(3, 672, 1_000);
        assert_eq!(Header::parse(&sent.encode()).expect("valid"), sent);
        let mut wrong_rate = sent;
        wrong_rate.rate = 10_000_000;
        assert!(Header::parse(&wrong_rate.encode()).is_err());
        let mut short = sent.encode();
        short.pop();
        assert!(Header::parse(&short).is_err());
        let mut ten_bit = sent.encode();
        ten_bit[6] = 10;
        assert!(Header::parse(&ten_bit).is_err());
        let mut foreign = sent.encode();
        foreign[3] = b'1';
        assert!(Header::parse(&foreign).is_err());
    }

    #[test]
    fn consecutive_packets_carry_no_gap() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.place(header(1, 0, 0)), Placed::Next);
        assert_eq!(tracker.place(header(1, 672, 42)), Placed::Next);
    }

    #[test]
    fn a_lost_packet_is_an_exact_gap_and_a_late_one_is_stale() {
        let mut tracker = Tracker::default();
        tracker.place(header(1, 0, 0));
        assert_eq!(
            tracker.place(header(1, 2_016, 126)),
            Placed::After(BlockGap {
                exact: 1_344,
                estimated: 0
            })
        );
        assert_eq!(tracker.place(header(1, 672, 42)), Placed::Stale);
    }

    #[test]
    fn a_new_epoch_estimates_the_quiet_time_from_the_clock() {
        let mut tracker = Tracker::default();
        tracker.place(header(1, 0, 1_000));
        assert_eq!(
            tracker.place(header(2, 0, 51_042)),
            Placed::After(BlockGap {
                exact: 0,
                estimated: 800_000
            })
        );
        assert_eq!(tracker.place(header(1, 672, 1_042)), Placed::Stale);
    }
}
