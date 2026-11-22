use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::{
    AUDIO_RATE, Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
    check_range,
};
use sdrmm_dsp::FmDemod;
use sdrmm_modem::analog::subaudible::NrzReceiver;
use sdrmm_modem::analog::voice::FmVoice;
use sdrmm_modem::cpm::Mapping;
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, DvFrame,
    DvFrameKind, EventKind, FilterSpec, KindSpec, SettingField,
};

mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
mod trunk;
mod word;

pub use model::*;
use word::{Meaning, WORD_BITS, Word};

const INPUT_RATE_HZ: f64 = 48_000.0;
const BANDWIDTH_HZ: f64 = 12_500.0;
const WIDTHS_HZ: [f64; 2] = [12_500.0, 25_000.0];
const CHANNEL_TAPS: usize = 129;
pub(crate) const BAUD: f64 = 300.0;
/// Peak voice deviation is a fifth of the channel width.
const DEVIATION_RATIO: f64 = 5.0;
/// How long a repeater may go without a word before its call counts as over: three words.
const WORD_HOLD_S: f64 = 0.4;
/// Boundary words a locked stream may miss before it is searched for afresh.
const MAX_MISSES: u32 = 2;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ltr".to_owned(),
    name: "LTR".to_owned(),
    summary: "LTR trunked repeater voice and data".to_owned(),
    family: DecoderFamily::DIGITAL_VOICE,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::DV],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<LtrParams, ChannelError> {
    let params = sdrmm_decoder::settings_of::<LtrParams>(settings)?;
    check_width(&params)?;
    Ok(params)
}

fn check_width(params: &LtrParams) -> Result<(), ChannelError> {
    check_range(
        "LTR bandwidth",
        "Hz",
        params.bandwidth_hz,
        WIDTHS_HZ[0]..=WIDTHS_HZ[1],
    )
}

fn occupied_band(params: &LtrParams) -> (f64, f64) {
    (-params.bandwidth_hz / 2.0, params.bandwidth_hz / 2.0)
}

fn channel_filter(params: &LtrParams) -> Result<FilterSpec, ChannelError> {
    check_width(params)?;
    Ok(FilterSpec::lowpass(CHANNEL_TAPS, params.bandwidth_hz / 2.0))
}

pub(crate) fn deviation_hz(params: &LtrParams) -> f64 {
    params.bandwidth_hz / DEVIATION_RATIO
}

pub struct LtrChannel {
    discriminator: FmDemod,
    detected: Vec<f32>,
    data: NrzReceiver,
    soft: Vec<f32>,
    rail: Mapping,
    framer: Framer,
    voice: FmVoice,
    call: CallState,
}

impl NativeChannel for LtrChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            discriminator: FmDemod::new(INPUT_RATE_HZ, deviation_hz(&params)),
            detected: Vec::new(),
            data: NrzReceiver::new(INPUT_RATE_HZ, BAUD),
            soft: Vec::new(),
            rail: Mapping::natural(2),
            framer: Framer::default(),
            voice: FmVoice::new(INPUT_RATE_HZ),
            call: CallState::default(),
        })
    }
}

impl Channel for LtrChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        self.discriminator = FmDemod::new(INPUT_RATE_HZ, deviation_hz(&params));
        Ok(())
    }

    fn retuned(&mut self) {
        self.data.reset();
        self.framer = Framer::default();
        self.voice.reset();
        self.call = CallState::default();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.discriminator.process(iq, &mut self.detected);
        self.soft.clear();
        self.data.soft(&self.detected, &mut self.soft);
        out.symbols.levels(&self.soft, &[], &self.rail, BAUD, 0.0);
        for &level in &self.soft {
            if let Some(word) = self.framer.push(level >= 0.0) {
                self.call.heard(word, out);
            }
        }
        self.call.elapse(iq.len(), out);
        self.voice.process(&self.detected, &mut out.audio_pcm);
        if !self.call.open() {
            out.audio_pcm.fill(0.0);
        }
        if !out.audio_pcm.is_empty() {
            out.audio_rate = AUDIO_RATE;
        }
    }

    fn rejected(&self) -> u32 {
        self.framer.rejected
    }
}

/// Finds words in the bit stream. A word found alone could be chance, so the stream locks
/// only on a second one right behind it, and from then on reads every word boundary.
#[derive(Default)]
struct Framer {
    register: u64,
    /// Bits until the next word boundary, while a word was found or the stream is locked.
    countdown: u32,
    locked: bool,
    misses: u32,
    rejected: u32,
}

impl Framer {
    fn push(&mut self, bit: bool) -> Option<Word> {
        self.register = self.register << 1 | u64::from(bit);
        if self.countdown > 1 {
            self.countdown -= 1;
            return None;
        }
        let at_boundary = self.countdown == 1;
        self.countdown = 0;
        let word = Word::read(self.register);
        match (word, at_boundary) {
            (Some(word), true) => {
                self.countdown = WORD_BITS;
                self.locked = true;
                self.misses = 0;
                Some(word)
            }
            (Some(_), false) => {
                self.countdown = WORD_BITS;
                None
            }
            (None, true) => {
                if self.locked {
                    self.rejected = self.rejected.saturating_add(1);
                    self.misses += 1;
                    self.locked = self.misses < MAX_MISSES;
                    if self.locked {
                        self.countdown = WORD_BITS;
                    }
                }
                None
            }
            (None, false) => None,
        }
    }
}

/// The call a repeater is carrying, as its words tell it, and whether its voice should play.
#[derive(Default)]
struct CallState {
    since_word: usize,
    current: Option<Word>,
    reported: Option<(Meaning, Word)>,
}

impl CallState {
    fn open(&self) -> bool {
        self.current.is_some()
    }

    fn heard(&mut self, word: Word, out: &mut ChannelOutputs) {
        self.since_word = 0;
        let meaning = word.meaning();
        match meaning {
            Meaning::Call => self.current = Some(word),
            Meaning::TurnOff => {
                if self.current.take().is_some() {
                    out.events.push(DecoderEvent::of(&frame(meaning, word)));
                }
                self.reported = None;
                return;
            }
            Meaning::Idle | Meaning::Other => self.current = None,
        }
        if self
            .reported
            .is_some_and(|(said, last)| said == meaning && same_call(last, word))
        {
            return;
        }
        self.reported = Some((meaning, word));
        out.events.push(DecoderEvent::of(&frame(meaning, word)));
    }

    fn elapse(&mut self, samples: usize, out: &mut ChannelOutputs) {
        self.since_word = self.since_word.saturating_add(samples);
        if self.since_word < (WORD_HOLD_S * INPUT_RATE_HZ) as usize {
            return;
        }
        self.reported = None;
        if let Some(word) = self.current.take() {
            out.events
                .push(DecoderEvent::of(&frame(Meaning::TurnOff, word)));
        }
    }
}

/// Whether two words tell of the same call; the free repeater changes from word to word.
fn same_call(a: Word, b: Word) -> bool {
    Word { free: 0, ..a } == Word { free: 0, ..b }
}

fn frame(meaning: Meaning, word: Word) -> DvFrame {
    let kind = match meaning {
        Meaning::Call => DvFrameKind::Header,
        Meaning::TurnOff => DvFrameKind::Terminator,
        Meaning::Idle | Meaning::Other => DvFrameKind::Control,
    };
    let mut frame = trunk::frame(kind);
    frame.opcode = Some(
        match (meaning, word.inbound) {
            (Meaning::Call, false) => "Call",
            (Meaning::Call, true) => "Radio call",
            (Meaning::TurnOff, _) => "Call end",
            (Meaning::Idle, _) => "Idle",
            (Meaning::Other, _) => "Unknown",
        }
        .to_owned(),
    );
    frame.system_id = Some(u16::from(word.area));
    frame.rest_channel = Some(u16::from(word.free)).filter(|&free| free != 0);
    if meaning == Meaning::Idle {
        frame.channel = Some(u16::from(word.goto));
        return frame;
    }
    frame.channel =
        Some(u16::from(word.goto)).filter(|&goto| goto <= u16::from(word::MAX_REPEATER));
    frame.destination = Some(word.talkgroup());
    frame.destination_call = Some(word.talkgroup_name());
    frame.group_call = Some(true);
    frame
}

pub(crate) fn ltr_decoder() -> Native<LtrParams> {
    Native::new::<LtrChannel>(occupied_band)
        .filter(channel_filter)
        .form(ltr_form)
        .kinds(ltr_kinds)
        .trunking(trunk::trunking())
}

pub(crate) fn ltr_form() -> Vec<SettingField> {
    vec![SettingField::bandwidth(&WIDTHS_HZ)]
}

fn ltr_kinds() -> Vec<KindSpec> {
    Vec::new()
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
