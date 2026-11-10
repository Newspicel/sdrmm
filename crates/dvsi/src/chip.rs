use std::time::{Duration, Instant};

use sdrmm_channels::chip::{ChipRate, ChipSpeech};

use crate::{
    ChipError,
    protocol::{self, Deframer, Packet, PacketKind, field, init},
    transport::{Opener, Transport},
};

/// The rates DVSI ships its boards at: the AMBE-3000 sticks first, then the USB-3003.
pub const BAUDS: [u32; 2] = [460_800, 921_600];
const READY_TIMEOUT: Duration = Duration::from_millis(500);
const REPLY_TIMEOUT: Duration = Duration::from_millis(400);
const READ_SLICE: Duration = Duration::from_millis(20);
const MULTI_CHANNEL_PRODUCT: &str = "3003";
const MULTI_CHANNEL_COUNT: u8 = 3;

/// What a chip said about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipInfo {
    pub product: String,
    pub version: String,
    pub channels: u8,
    pub baud: u32,
}

/// A chip answering in packet mode.
pub struct Chip {
    port: Box<dyn Transport>,
    deframer: Deframer,
    info: ChipInfo,
}

impl Chip {
    /// The chip on `port` at the first of `bauds` it answers on.
    pub fn probe(opener: &dyn Opener, port: &str, bauds: &[u32]) -> Result<Self, ChipError> {
        let mut failures = Vec::new();
        for &baud in bauds {
            match opener
                .open(port, baud)
                .map_err(ChipError::Open)
                .and_then(|transport| Self::identify(transport, baud))
            {
                Ok(chip) => return Ok(chip),
                Err(ChipError::Open(error)) => return Err(ChipError::Open(error)),
                Err(error) => failures.push(format!("{baud} baud: {error}")),
            }
        }
        Err(ChipError::Silent(failures.join("; ")))
    }

    fn identify(port: Box<dyn Transport>, baud: u32) -> Result<Self, ChipError> {
        let mut chip = Self {
            port,
            deframer: Deframer::default(),
            info: ChipInfo {
                product: String::new(),
                version: String::new(),
                channels: 1,
                baud,
            },
        };
        chip.port.discard_input()?;
        chip.reset()?;
        let product = chip.ask_text(field::PRODUCT_ID)?;
        let version = chip.ask_text(field::VERSION)?;
        chip.info.channels = if product.contains(MULTI_CHANNEL_PRODUCT) {
            MULTI_CHANNEL_COUNT
        } else {
            1
        };
        chip.info.product = product;
        chip.info.version = version;
        Ok(chip)
    }

    #[must_use]
    pub const fn info(&self) -> &ChipInfo {
        &self.info
    }

    /// Restarts the chip; one that never says it is ready still gets asked who it is.
    fn reset(&mut self) -> Result<(), ChipError> {
        self.send(PacketKind::Control, &[field::RESET])?;
        match self.receive(PacketKind::Control, READY_TIMEOUT) {
            Ok(packet) if packet.fields.first() == Some(&field::READY) => Ok(()),
            Ok(_) | Err(ChipError::Silent(_)) => {
                self.deframer.clear();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn ask_text(&mut self, id: u8) -> Result<String, ChipError> {
        let packet = self.exchange(PacketKind::Control, &[id], PacketKind::Control)?;
        protocol::text(&packet.fields, id)
    }

    fn addressed(&self, channel: u8, fields: &[u8]) -> Vec<u8> {
        protocol::channel_field(self.info.channels, channel)
            .into_iter()
            .chain(fields.iter().copied())
            .collect()
    }

    fn control(&mut self, channel: u8, fields: &[u8], ids: &[u8]) -> Result<(), ChipError> {
        let request = self.addressed(channel, fields);
        let answer = self.exchange(PacketKind::Control, &request, PacketKind::Control)?;
        let expected: Vec<u8> = protocol::channel_field(self.info.channels, channel)
            .into_iter()
            .chain(ids.iter().copied())
            .collect();
        protocol::statuses(&answer.fields, &expected)
    }

    /// Sets `channel` to `rate` and restarts its encoder and decoder.
    pub fn configure(&mut self, channel: u8, rate: ChipRate) -> Result<(), ChipError> {
        let mut fields = protocol::rate_fields(rate);
        let rate_id = fields[0];
        fields.extend([field::INIT, init::BOTH]);
        self.control(channel, &fields, &[rate_id, field::INIT])
    }

    /// Restarts the decoder of `channel` between calls.
    pub fn restart_decoder(&mut self, channel: u8) -> Result<(), ChipError> {
        self.control(channel, &[field::INIT, init::DECODER], &[field::INIT])
    }

    /// The speech of one frame of `rate` bits, MSB first.
    pub fn decode(
        &mut self,
        channel: u8,
        rate: ChipRate,
        bits: &[u8],
    ) -> Result<ChipSpeech, ChipError> {
        let length = rate.frame_bytes().min(bits.len());
        let mut fields = vec![field::CHANNEL_DATA, rate.frame_bits()];
        fields.extend_from_slice(&bits[..length]);
        let request = self.addressed(channel, &fields);
        let answer = self.exchange(PacketKind::Channel, &request, PacketKind::Speech)?;
        let (from, speech) = protocol::speech(&answer.fields)?;
        match from {
            Some(from) if from != channel => Err(ChipError::Protocol(format!(
                "speech from channel {from}, asked channel {channel}"
            ))),
            _ => Ok(speech),
        }
    }

    fn send(&mut self, kind: PacketKind, fields: &[u8]) -> Result<(), ChipError> {
        self.port.send(&protocol::encode(kind, fields))?;
        Ok(())
    }

    fn exchange(
        &mut self,
        kind: PacketKind,
        fields: &[u8],
        answer: PacketKind,
    ) -> Result<Packet, ChipError> {
        self.send(kind, fields)?;
        self.receive(answer, REPLY_TIMEOUT)
    }

    /// The next packet of `kind`, skipping others, within `timeout`.
    fn receive(&mut self, kind: PacketKind, timeout: Duration) -> Result<Packet, ChipError> {
        let deadline = Instant::now() + timeout;
        let mut buffer = [0u8; 512];
        loop {
            while let Some(packet) = self.deframer.next_packet() {
                let packet = packet?;
                if packet.kind == kind {
                    return Ok(packet);
                }
                tracing::debug!(?packet.kind, "dvsi chip sent an unasked packet");
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ChipError::Silent(format!(
                    "no answer in {} ms",
                    timeout.as_millis()
                )));
            }
            let read = self.port.receive(&mut buffer, left.min(READ_SLICE))?;
            self.deframer.push(&buffer[..read]);
        }
    }
}
