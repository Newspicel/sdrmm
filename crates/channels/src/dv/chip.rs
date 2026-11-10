//! A hardware vocoder chip the voice decoders hand their frames to in place of blip25.
//!
//! The decoder side is lock free: frames go out and speech comes back through ring buffers a
//! worker on the far side services. While the chip cannot take a frame, the decoder plays it in
//! software and keeps sending it so the chip stays in step and can take over again.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU8, Ordering},
};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::ChannelError;

/// Whether this build offers vocoder chips: the `vocoder-chip` feature, which a host that
/// installs [`VocoderChips`] turns on.
pub const VOCODER_CHIPS: bool = cfg!(feature = "vocoder-chip");
pub const CHIP_FRAME_SAMPLES: usize = 160;
pub const MAX_CHIP_FRAME_BYTES: usize = 11;
const QUEUE_FRAMES: usize = 32;

pub type ChipSpeech = [i16; CHIP_FRAME_SAMPLES];

/// The codec one link carries, as the chip is configured for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChipRate {
    /// AMBE+2 half rate: the 49 information bits, FEC already undone.
    Half,
    /// IMBE full rate: the 88 information bits, FEC already undone.
    Full,
    /// D-STAR AMBE: the 72 bits of the voice frame, FEC included, in air byte order.
    Dstar,
}

impl ChipRate {
    #[must_use]
    pub const fn frame_bits(self) -> u8 {
        match self {
            Self::Half => 49,
            Self::Full => 88,
            Self::Dstar => 72,
        }
    }

    #[must_use]
    pub const fn frame_bytes(self) -> usize {
        (self.frame_bits() as usize).div_ceil(8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipFrame {
    /// `played` marks a frame the decoder already played in software: the chip decodes it to
    /// stay in step and its speech is dropped.
    Voice {
        bits: [u8; MAX_CHIP_FRAME_BYTES],
        played: bool,
    },
    Silence,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ChipHealth {
    Ready = 0,
    /// Every channel of the chip is decoding another call.
    Busy = 1,
    /// The chip stopped answering.
    Lost = 2,
}

impl ChipHealth {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Ready,
            1 => Self::Busy,
            _ => Self::Lost,
        }
    }
}

/// The decoder's end of a link.
pub struct ChipLink {
    rate: ChipRate,
    name: Arc<str>,
    frames: Producer<ChipFrame>,
    speech: Consumer<ChipSpeech>,
    health: Arc<AtomicU8>,
}

impl ChipLink {
    #[must_use]
    pub const fn rate(&self) -> ChipRate {
        self.rate
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn health(&self) -> ChipHealth {
        if self.frames.is_abandoned() {
            return ChipHealth::Lost;
        }
        ChipHealth::from_raw(self.health.load(Ordering::Acquire))
    }

    /// Whether the frame went out; a full queue means the worker stalled.
    pub fn send(&mut self, frame: ChipFrame) -> bool {
        self.frames.push(frame).is_ok()
    }

    pub fn receive(&mut self) -> Option<ChipSpeech> {
        self.speech.pop().ok()
    }
}

/// The chip worker's end of a link.
pub struct ChipEnd {
    rate: ChipRate,
    frames: Consumer<ChipFrame>,
    speech: Producer<ChipSpeech>,
    health: Arc<AtomicU8>,
}

impl ChipEnd {
    #[must_use]
    pub const fn rate(&self) -> ChipRate {
        self.rate
    }

    pub fn next_frame(&mut self) -> Option<ChipFrame> {
        self.frames.pop().ok()
    }

    /// Hands speech back; `false` when the decoder stopped draining it.
    pub fn deliver(&mut self, speech: ChipSpeech) -> bool {
        self.speech.push(speech).is_ok()
    }

    pub fn set_health(&self, health: ChipHealth) {
        self.health.store(health as u8, Ordering::Release);
    }

    #[must_use]
    pub fn abandoned(&self) -> bool {
        self.frames.is_abandoned()
    }
}

/// A link carrying `rate`, its decoder end reporting under `name`.
#[must_use]
pub fn chip_link(rate: ChipRate, name: &str) -> (ChipLink, ChipEnd) {
    let (frame_tx, frame_rx) = RingBuffer::new(QUEUE_FRAMES);
    let (speech_tx, speech_rx) = RingBuffer::new(QUEUE_FRAMES);
    let health = Arc::new(AtomicU8::new(ChipHealth::Ready as u8));
    (
        ChipLink {
            rate,
            name: name.into(),
            frames: frame_tx,
            speech: speech_rx,
            health: Arc::clone(&health),
        },
        ChipEnd {
            rate,
            frames: frame_rx,
            speech: speech_tx,
            health,
        },
    )
}

/// Opens vocoder chips by serial port, on the control plane.
pub trait VocoderChips: Send + Sync {
    fn open(&self, port: &str, rate: ChipRate) -> Result<ChipLink, String>;
}

static CHIPS: OnceLock<Arc<dyn VocoderChips>> = OnceLock::new();

/// Makes `chips` the way every decoder opens a vocoder chip; `false` when one already is.
pub fn install_vocoder_chips(chips: Arc<dyn VocoderChips>) -> bool {
    CHIPS.set(chips).is_ok()
}

/// The chip on `port` for `rate`, or none for software synthesis.
pub(crate) fn open_chip(
    port: Option<&str>,
    rate: ChipRate,
) -> Result<Option<ChipLink>, ChannelError> {
    let Some(port) = port.filter(|port| !port.is_empty()) else {
        return Ok(None);
    };
    let chips = CHIPS.get().ok_or_else(|| {
        ChannelError::Unsupported("this build has no vocoder chip support".to_owned())
    })?;
    chips
        .open(port, rate)
        .map(Some)
        .map_err(|error| ChannelError::InvalidSettings(format!("vocoder chip on {port}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_speech_cross_the_link_in_order() {
        let (mut link, mut end) = chip_link(ChipRate::Half, "test");
        assert!(link.send(ChipFrame::Silence));
        assert!(link.send(ChipFrame::Reset));
        assert_eq!(end.next_frame(), Some(ChipFrame::Silence));
        assert_eq!(end.next_frame(), Some(ChipFrame::Reset));
        assert!(end.deliver([7; CHIP_FRAME_SAMPLES]));
        assert_eq!(link.receive(), Some([7; CHIP_FRAME_SAMPLES]));
        assert_eq!(link.receive(), None);
    }

    #[test]
    fn health_follows_the_worker_and_its_departure() {
        let (link, end) = chip_link(ChipRate::Full, "test");
        assert_eq!(link.health(), ChipHealth::Ready);
        end.set_health(ChipHealth::Busy);
        assert_eq!(link.health(), ChipHealth::Busy);
        drop(end);
        assert_eq!(link.health(), ChipHealth::Lost);
    }

    #[test]
    fn a_stalled_worker_fills_the_queue_instead_of_blocking() {
        let (mut link, _end) = chip_link(ChipRate::Dstar, "test");
        let sent = (0..QUEUE_FRAMES + 4)
            .filter(|_| link.send(ChipFrame::Silence))
            .count();
        assert_eq!(sent, QUEUE_FRAMES);
    }

    #[test]
    fn no_port_means_software() {
        assert!(matches!(open_chip(None, ChipRate::Half), Ok(None)));
        assert!(matches!(open_chip(Some(""), ChipRate::Half), Ok(None)));
    }

    #[test]
    fn decoders_offer_a_chip_only_in_builds_that_drive_one() {
        let offers =
            |form: Vec<sdrmm_wire::SettingField>| form.iter().any(|field| field.key == "vocoder");
        assert_eq!(offers(crate::dv::p25_form()), VOCODER_CHIPS);
        assert_eq!(offers(crate::dv::dmr::dmr_form()), VOCODER_CHIPS);
    }

    #[test]
    fn frame_sizes_match_the_codecs() {
        assert_eq!(ChipRate::Half.frame_bytes(), 7);
        assert_eq!(ChipRate::Full.frame_bytes(), MAX_CHIP_FRAME_BYTES);
        assert_eq!(ChipRate::Dstar.frame_bytes(), 9);
    }
}
