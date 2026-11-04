use crate::registry::Native;
use sdrmm_wire::{Control, EventContract, KindSpec, NumberSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, FmDemod, RealDecimator, design_lowpass};
#[cfg(any(test, feature = "synth"))]
use sdrmm_modem::cpm::{CpmParams, Mapping};
use sdrmm_modem::ita2::{self, ShiftDecoder};
use sdrmm_modem::pulse::{self, Norm};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate,
};

const CHANNEL_TAPS: usize = 257;

const MAX_POST_TAPS: usize = 511;

const MIN_LEVEL: f32 = 0.5;

const FLUSH_CHARS: usize = 72;

const IDLE_FLUSH_BITS: f64 = 24.0;

const FRAME_BITS: usize = 7;
const STOP_INDEX: usize = FRAME_BITS - 1;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "rtty".to_owned(),
    name: "RTTY".to_owned(),
    summary: "Radioteletype text".to_owned(),
    family: DecoderFamily::Amateur,
    bandwidth_hz: 1_000.0,
    input_rate_hz: 8_000.0,
    has_audio: false,
    emits: vec![RttyText::KIND],
    ..ChannelDescriptor::default()
});

pub struct RttyChannel {
    demod: FmDemod,
    post: RealDecimator,
    polarity: f32,
    decoder: Decoder,
    demod_buf: Vec<f32>,
    filtered: Vec<f32>,
}

fn params(settings: &ChannelSettings) -> Result<RttyParams, ChannelError> {
    crate::settings_of::<RttyParams>(settings)
}

fn check_params(p: &RttyParams) -> Result<(), ChannelError> {
    let rate = DESCRIPTOR.input_rate_hz;
    if !(p.baud.is_finite() && p.baud > 0.0 && p.baud < rate / 4.0) {
        return Err(ChannelError::InvalidSettings(format!(
            "rtty baud must be in (0, {}), got {}",
            rate / 4.0,
            p.baud
        )));
    }
    if !(p.shift_hz.is_finite() && p.shift_hz > 0.0 && p.shift_hz < rate / 4.0) {
        return Err(ChannelError::InvalidSettings(format!(
            "rtty shift must be in (0, {}) Hz, got {}",
            rate / 4.0,
            p.shift_hz
        )));
    }
    Ok(())
}

pub(crate) fn occupied_band(p: &RttyParams) -> (f64, f64) {
    let half = p.shift_hz / 2.0 + 2.0 * p.baud;
    (-half, half)
}

pub(crate) fn channel_filter(p: &RttyParams) -> Result<ChannelFilter, ChannelError> {
    check_params(p)?;
    let (_, half) = occupied_band(p);
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, half / DESCRIPTOR.input_rate_hz),
        1,
    )))
}

fn post_filter(p: &RttyParams) -> RealDecimator {
    let sps = (DESCRIPTOR.input_rate_hz / p.baud).min(MAX_POST_TAPS as f64);
    RealDecimator::new(&pulse::rect(sps, Norm::Area), 1)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn cell_params(baud: f64, shift_hz: f64, rate: f64) -> CpmParams {
    let cell_baud = 2.0 * baud;
    let sps = rate / cell_baud;
    CpmParams::from_deviation(
        Mapping::new(vec![-1.0, 1.0]),
        shift_hz / 2.0,
        cell_baud,
        pulse::rect(sps, Norm::Area),
        sps,
    )
}

impl NativeChannel for RttyChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = &params(&settings)?;
        check_params(p)?;
        Ok(Self {
            demod: FmDemod::new(ctx.input_rate, p.shift_hz / 2.0),
            post: post_filter(p),
            polarity: polarity(p),
            decoder: Decoder::new(ctx.input_rate, p),
            demod_buf: Vec::new(),
            filtered: Vec::new(),
        })
    }
}

impl ChannelRx for RttyChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = &params(&settings)?;
        check_params(p)?;
        let rate = DESCRIPTOR.input_rate_hz;
        self.demod = FmDemod::new(rate, p.shift_hz / 2.0);
        self.post = post_filter(p);
        self.polarity = polarity(p);
        self.decoder.configure(rate, p);
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.demod.process(iq, &mut self.demod_buf);
        for s in &mut self.demod_buf {
            *s = if s.is_finite() {
                (*s * self.polarity).clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        self.post.process(&self.demod_buf, &mut self.filtered);
        for &level in &self.filtered {
            self.decoder.feed(level, out);
        }
    }
}

fn polarity(p: &RttyParams) -> f32 {
    if p.invert { -1.0 } else { 1.0 }
}

#[derive(Clone, Copy, Default)]
struct Frame {
    since_edge: usize,
    next: usize,
    bits: u8,
}

struct Decoder {
    slice_at: [usize; FRAME_BITS],
    idle_flush: usize,
    unshift_on_space: bool,
    positive: bool,
    frame: Option<Frame>,
    ita2: ShiftDecoder,
    text: String,
    newline: bool,
    since_char: usize,
}

impl Decoder {
    fn new(rate: f64, p: &RttyParams) -> Self {
        let mut decoder = Self {
            slice_at: [0; FRAME_BITS],
            idle_flush: 0,
            unshift_on_space: true,
            positive: true,
            frame: None,
            ita2: ShiftDecoder::new(&ita2::FIGURES_US),
            text: String::new(),
            newline: false,
            since_char: 0,
        };
        decoder.configure(rate, p);
        decoder
    }

    fn configure(&mut self, rate: f64, p: &RttyParams) {
        let sps = rate / p.baud;
        for (i, at) in self.slice_at.iter_mut().enumerate() {
            *at = ((i as f64 + 0.5) * sps).round() as usize;
        }
        self.idle_flush = (IDLE_FLUSH_BITS * sps) as usize;
        self.unshift_on_space = p.unshift_on_space;
        self.frame = None;
    }

    fn feed(&mut self, level: f32, out: &mut ChannelOutputs) {
        self.since_char = self.since_char.saturating_add(1);
        if self.since_char == self.idle_flush {
            self.flush(out);
        }
        if let Some(mut frame) = self.frame {
            frame.since_edge += 1;
            self.frame = if frame.since_edge == self.slice_at[frame.next] {
                self.slice(frame, level, out)
            } else {
                Some(frame)
            };
        } else if self.positive && level < 0.0 {
            self.frame = Some(Frame::default());
        }
        self.positive = level >= 0.0;
    }

    fn slice(&mut self, mut frame: Frame, level: f32, out: &mut ChannelOutputs) -> Option<Frame> {
        match frame.next {
            0 => {
                if level > -MIN_LEVEL {
                    return None;
                }
            }
            STOP_INDEX => {
                if level > MIN_LEVEL {
                    self.emit(frame.bits, out);
                }
                return None;
            }
            i => {
                if level.abs() < MIN_LEVEL {
                    return None;
                }
                if level > 0.0 {
                    frame.bits |= 1 << (i - 1);
                }
            }
        }
        frame.next += 1;
        Some(frame)
    }

    fn emit(&mut self, code: u8, out: &mut ChannelOutputs) {
        self.since_char = 0;
        let Some(ch) = self.ita2.decode(code) else {
            return;
        };
        if code == ita2::SPACE && self.unshift_on_space {
            self.ita2.unshift();
        }
        match ch {
            '\0' => {}
            '\r' | '\n' => {
                if !self.newline {
                    self.text.push('\n');
                    self.newline = true;
                    self.flush(out);
                }
            }
            c => {
                self.newline = false;
                self.text.push(c);
                if self.text.len() >= FLUSH_CHARS {
                    self.flush(out);
                }
            }
        }
    }

    fn flush(&mut self, out: &mut ChannelOutputs) {
        if self.text.is_empty() {
            return;
        }
        out.events.push(DecoderEvent::of(&RttyText {
            text: self.text.clone(),
        }));
        self.text.clear();
    }
}

fn rtty_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "RTTY",
            &[Modulation::Fsk2],
            HzRange::new(100.0, 2_500.0),
            "a slow two-tone shift, teleprinter speed",
        )
        .symbol_rate(HzRange::new(40.0, 110.0))
        .deviation(HzRange::new(40.0, 500.0)),
    ]
}

pub(crate) fn rtty_decoder() -> Native<RttyParams> {
    Native::new::<RttyChannel>(occupied_band)
        .filter(channel_filter)
        .signatures(rtty_signatures)
        .form(rtty_form)
        .kinds(rtty_kinds)
}

pub(crate) fn rtty_form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "baud",
            "Baud",
            NumberSpec::new(10.0, 1_200.0, 0.05).presets(&RTTY_BAUDS, |baud| format!("{baud}")),
        )
        .title("RTTY baud"),
        SettingField::new(
            "shift_hz",
            "Shift",
            NumberSpec::hz(20.0, 2_000.0, 5.0).presets(&RTTY_SHIFTS_HZ, |hz| format!("{hz}")),
        )
        .title("RTTY shift"),
        SettingField::new("stop_bits", "Stop", Control::choice::<RttyStopBits>())
            .title("RTTY stop bits"),
        SettingField::invert(),
        SettingField::new("unshift_on_space", "Unshift", Control::Toggle)
            .title("Drop back to letters after a space, as most stations expect"),
    ]
}

fn rtty_kinds() -> Vec<KindSpec> {
    vec![rtty_kind()]
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::ChannelParams;

    use super::*;
    use crate::{
        nfm::NfmParams,
        synth::{
            self,
            rtty::{encode_codes, modulate, transmission},
        },
        testutil::{complex_noise, settings},
    };

    const RATE: f64 = 8_000.0;
    const BLOCKS: [usize; 7] = [997, 1, 4_096, 65, 2_048, 7, 1_024];

    fn channel(p: RttyParams) -> RttyChannel {
        RttyChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::of(&p).expect("rtty settings")),
        )
        .unwrap()
    }

    fn tail(baud: f64) -> Vec<Complex<f32>> {
        synth::silence((40.0 * RATE / baud) as usize)
    }

    fn decode_blocks(chan: &mut RttyChannel, iq: &[Complex<f32>], lens: &[usize]) -> Vec<String> {
        let mut out = ChannelOutputs::default();
        let mut texts = Vec::new();
        let mut pos = 0;
        for len in lens.iter().cycle() {
            if pos >= iq.len() {
                break;
            }
            let end = (pos + len).min(iq.len());
            out.reset();
            chan.process(&iq[pos..end], &mut out);
            assert!(out.audio_pcm.is_empty(), "rtty must not produce audio");
            for ev in &out.events {
                match ev.read::<RttyText>() {
                    Some(t) => texts.push(t.text),
                    None => panic!("unexpected event {ev:?}"),
                }
            }
            pos = end;
        }
        texts
    }

    fn decode(p: RttyParams, iq: &[Complex<f32>]) -> String {
        decode_blocks(&mut channel(p), iq, &BLOCKS).concat()
    }

    fn burst(text: &str, p: &RttyParams) -> Vec<Complex<f32>> {
        let mut iq = transmission(text, p.baud, p.shift_hz, p.stop_bits.periods(), RATE);
        iq.extend_from_slice(&tail(p.baud));
        iq
    }

    #[test]
    fn decodes_an_amateur_call_at_45_baud_170_shift() {
        let p = RttyParams::default();
        assert_eq!(p.baud, 45.45);
        assert_eq!(p.shift_hz, 170.0);
        assert_eq!(p.stop_bits, RttyStopBits::OneAndHalf);
        let texts = decode_blocks(
            &mut channel(p.clone()),
            &burst("CQ CQ DE DL1ABC K", &p),
            &BLOCKS,
        );
        assert_eq!(texts, vec!["CQ CQ DE DL1ABC K".to_owned()]);
    }

    #[test]
    fn decodes_figures_through_the_shift_codes() {
        let p = RttyParams::default();
        assert_eq!(decode(p.clone(), &burst("599 001", &p)), "599 001");
    }

    #[test]
    fn decodes_commercial_baud_and_shift_combinations() {
        for (baud, shift_hz, stop_bits) in [
            (50.0, 450.0, RttyStopBits::One),
            (75.0, 850.0, RttyStopBits::Two),
            (45.45, 850.0, RttyStopBits::OneAndHalf),
        ] {
            let p = RttyParams {
                baud,
                shift_hz,
                stop_bits,
                ..RttyParams::default()
            };
            let got = decode(p.clone(), &burst("RYRY DE SDR 123", &p));
            assert_eq!(got, "RYRY DE SDR 123", "{baud} baud / {shift_hz} Hz");
        }
    }

    #[test]
    fn crlf_collapses_to_one_line_break_and_ends_the_event() {
        let p = RttyParams::default();
        let texts = decode_blocks(&mut channel(p.clone()), &burst("AB\r\r\nCD", &p), &BLOCKS);
        assert_eq!(texts, vec!["AB\n".to_owned(), "CD".to_owned()]);
    }

    #[test]
    fn inverted_transmission_needs_the_invert_flag() {
        let p = RttyParams::default();
        let normal = burst("TEST DE RTTY", &p);
        let inverted: Vec<Complex<f32>> = normal.iter().map(Complex::conj).collect();

        let flagged = RttyParams {
            invert: true,
            ..p.clone()
        };
        assert_eq!(decode(flagged, &inverted), "TEST DE RTTY");
        assert_ne!(decode(p, &inverted), "TEST DE RTTY");
    }

    #[test]
    fn unshift_on_space_recovers_a_stream_that_lost_its_letters_shift() {
        let codes = [ita2::FIGS, 0x17, 0x07, ita2::SPACE, 0x03, 0x19, 0x0E];
        let p = RttyParams::default();
        let mut iq = modulate(
            &encode_codes(&codes, p.stop_bits.periods()),
            p.baud,
            p.shift_hz,
            RATE,
        );
        iq.extend_from_slice(&tail(p.baud));

        assert_eq!(decode(p.clone(), &iq), "17 ABC");
        let stay = RttyParams {
            unshift_on_space: false,
            ..p
        };
        assert_eq!(decode(stay, &iq), "17 -?:");
    }

    #[test]
    fn pure_noise_decodes_to_nothing() {
        let p = RttyParams::default();
        for seed in [0x1234_5678, 0xdead_beef, 0x0f0f_0f0f] {
            let noise = complex_noise(seed, 0.05, 120_000);
            assert_eq!(decode(p.clone(), &noise), "", "seed {seed:#x} raw");
            let noise = selected(&p, &noise);
            assert_eq!(decode(p.clone(), &noise), "", "seed {seed:#x} filtered");
        }
    }

    #[test]
    fn noise_around_a_transmission_adds_no_characters() {
        let p = RttyParams::default();
        let mut iq = complex_noise(0x5eed_1234, 0.02, 20_000);
        iq.extend_from_slice(&burst("DE DL1ABC", &p));
        iq.extend_from_slice(&complex_noise(0xfeed_face, 0.02, 20_000));
        assert_eq!(decode(p, &iq), "DE DL1ABC");
    }

    fn selected(p: &RttyParams, iq: &[Complex<f32>]) -> Vec<Complex<f32>> {
        let mut filtered = Vec::new();
        channel_filter(p).unwrap().process(iq, &mut filtered);
        filtered
    }

    #[test]
    fn decodes_through_additive_noise() {
        let p = RttyParams::default();
        let mut iq = burst("CQ DE DL1ABC", &p);
        synth::add_noise(&mut iq, 0xabad_1dea, 0.5);
        assert_eq!(decode(p.clone(), &selected(&p, &iq)), "CQ DE DL1ABC");
    }

    #[test]
    fn ragged_block_splits_decode_identically() {
        let p = RttyParams::default();
        let iq = burst("THE QUICK BROWN FOX 1234567890", &p);
        let whole = decode_blocks(&mut channel(p.clone()), &iq, &[iq.len()]);
        let ragged = decode_blocks(&mut channel(p.clone()), &iq, &BLOCKS);
        let single = decode_blocks(&mut channel(p), &iq, &[1]);
        assert_eq!(whole, vec!["THE QUICK BROWN FOX 1234567890".to_owned()]);
        assert_eq!(ragged, whole);
        assert_eq!(single, whole);
    }

    #[test]
    fn apply_switches_baud_and_shift_in_place() {
        let mut chan = channel(RttyParams::default());
        let p = RttyParams {
            baud: 75.0,
            shift_hz: 850.0,
            stop_bits: RttyStopBits::Two,
            ..RttyParams::default()
        };
        chan.apply(settings(ChannelParams::of(&p).expect("rtty settings")))
            .unwrap();
        let texts = decode_blocks(&mut chan, &burst("QRV 75 BAUD", &p), &BLOCKS);
        assert_eq!(texts, vec!["QRV 75 BAUD".to_owned()]);
    }

    #[test]
    fn out_of_range_params_are_rejected() {
        for p in [
            RttyParams {
                baud: 0.0,
                ..RttyParams::default()
            },
            RttyParams {
                baud: f64::NAN,
                ..RttyParams::default()
            },
            RttyParams {
                baud: 2_000.0,
                ..RttyParams::default()
            },
            RttyParams {
                shift_hz: 0.0,
                ..RttyParams::default()
            },
            RttyParams {
                shift_hz: 4_000.0,
                ..RttyParams::default()
            },
        ] {
            let built = RttyChannel::new(
                ChannelCtx { input_rate: RATE },
                settings(ChannelParams::of(&p).expect("rtty settings")),
            );
            assert!(
                matches!(built, Err(ChannelError::InvalidSettings(_))),
                "{p:?} must be rejected"
            );
            assert!(matches!(
                channel_filter(&p),
                Err(ChannelError::InvalidSettings(_))
            ));
        }
    }

    #[test]
    fn mismatched_params_variant_is_rejected() {
        let mut chan = channel(RttyParams::default());
        let err = chan.apply(settings(
            ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"),
        ));
        assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
        let built = RttyChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")),
        );
        assert!(matches!(built, Err(ChannelError::InvalidSettings(_))));
    }

    #[test]
    fn wrong_input_rate_is_rejected() {
        let built = RttyChannel::new(
            ChannelCtx {
                input_rate: 48_000.0,
            },
            settings(ChannelParams::of(&RttyParams::default()).expect("rtty settings")),
        );
        assert!(matches!(built, Err(ChannelError::InvalidSettings(_))));
    }
}
