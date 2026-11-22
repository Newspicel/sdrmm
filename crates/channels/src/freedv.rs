mod model;
pub use model::*;
use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::golay23_correct;
use sdrmm_modem::{
    constellation::{Constellation, tables},
    multicarrier::fdmdv::{self, Demodulator, Frame},
};
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, ClickProfile, DecoderEvent, DecoderFamily,
    DvFrame, DvFrameKind, EventKind, Sideband,
};

use sdrmm_codec::voice::Codec2Decoder;
use sdrmm_decoder::voice::Speaker;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};

const INPUT_RATE_HZ: f64 = 8_000.0;
const LOW_EDGE_HZ: f64 = 800.0;
const HIGH_EDGE_HZ: f64 = 2_200.0;
const FILTER_TAPS: usize = 257;
const MODEM_SCALE: f32 = 32_768.0 / 825.0;
const MODEM_BITS: usize = fdmdv::BITS;
const CODEC_BITS: usize = 52;
const TAP_POINTS: usize = 1024;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "freedv".to_owned(),
    name: "FreeDV 1600".to_owned(),
    summary: "FreeDV HF digital voice".to_owned(),
    family: DecoderFamily::DIGITAL_VOICE,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: HIGH_EDGE_HZ - LOW_EDGE_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::DV],
    calls: Some(CallGate::Frames),
    click: ClickProfile::Vocoder,
    ..ChannelDescriptor::default()
});

pub struct FreeDvChannel {
    sideband: Sideband,
    modem: Demodulator,
    paired_bits: [bool; MODEM_BITS * 2],
    even_frame: bool,
    synced: bool,
    vocoder: Codec2Decoder,
    rejected: u32,
    dqpsk: Constellation,
    points: Vec<Complex<f32>>,
}

fn dv_frame(kind: DvFrameKind) -> DvFrame {
    DvFrame::new("FreeDV", kind)
}

fn params(settings: &ChannelSettings) -> Result<FreeDvParams, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
}

pub(crate) fn occupied_band(params: FreeDvParams) -> (f64, f64) {
    match params.sideband {
        Sideband::Usb => (LOW_EDGE_HZ, HIGH_EDGE_HZ),
        Sideband::Lsb => (-HIGH_EDGE_HZ, -LOW_EDGE_HZ),
    }
}

pub(crate) fn channel_filter(params: FreeDvParams) -> FilterSpec {
    let (low, high) = occupied_band(params);
    FilterSpec::band(FILTER_TAPS, low, high)
}

impl NativeChannel for FreeDvChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            sideband: params.sideband,
            modem: Demodulator::new(),
            paired_bits: [false; MODEM_BITS * 2],
            even_frame: false,
            synced: false,
            vocoder: Codec2Decoder::new(),
            rejected: 0,
            dqpsk: tables::psk(4)
                .map_err(|error| ChannelError::InvalidSettings(error.to_string()))?,
            points: Vec::with_capacity(TAP_POINTS),
        })
    }
}

impl Channel for FreeDvChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let sideband = params(&settings)?.sideband;
        if sideband != self.sideband {
            self.modem.reset();
            self.sideband = sideband;
            self.reset_stream_state();
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.modem.reset();
        self.reset_stream_state();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.points.clear();
        for &sample in iq {
            let sample = match self.sideband {
                Sideband::Usb => sample,
                Sideband::Lsb => sample.conj(),
            } * MODEM_SCALE;
            if let Some(frame) = self.modem.push(sample) {
                self.keep_points(&frame, out);
                self.demod_frame(&frame, out);
            }
        }
        self.tap_symbols(out);
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }
}

impl FreeDvChannel {
    fn reset_stream_state(&mut self) {
        self.even_frame = false;
        self.synced = false;
        self.vocoder.reset();
    }

    fn keep_points(&mut self, frame: &Frame, out: &ChannelOutputs) {
        if out.symbols.wanted()
            && frame.sync
            && self.points.len() + frame.points.len() <= self.points.capacity()
        {
            self.points.extend_from_slice(&frame.points);
        }
    }

    fn tap_symbols(&self, out: &mut ChannelOutputs) {
        if self.points.is_empty() {
            return;
        }
        let offset = f64::from(self.modem.frequency_offset_hz());
        out.symbols.carriers(
            &self.points,
            &self.dqpsk,
            fdmdv::DATA_CARRIERS as f64 * fdmdv::SYMBOL_RATE_HZ,
            match self.sideband {
                Sideband::Usb => offset,
                Sideband::Lsb => -offset,
            },
        );
    }

    fn demod_frame(&mut self, frame: &Frame, out: &mut ChannelOutputs) {
        let sync = frame.sync;
        if sync && !self.synced {
            let mut header = dv_frame(DvFrameKind::Header);
            header.opcode = Some("1600".to_owned());
            out.events.push(DecoderEvent::of(&header));
        } else if !sync && self.synced {
            self.rejected = self.rejected.saturating_add(1);
            out.events
                .push(DecoderEvent::of(&dv_frame(DvFrameKind::Terminator)));
        }
        self.synced = sync;

        if frame.reliable_sync {
            self.even_frame = true;
        }
        if !sync {
            return;
        }
        let offset = if self.even_frame { MODEM_BITS } else { 0 };
        self.paired_bits[offset..offset + MODEM_BITS].copy_from_slice(&frame.bits);
        if self.even_frame {
            self.decode_voice(out);
        }
        self.even_frame = !self.even_frame;
    }

    fn decode_voice(&mut self, out: &mut ChannelOutputs) {
        let received = self.paired_bits[..8]
            .iter()
            .chain(&self.paired_bits[11..15])
            .chain(&self.paired_bits[CODEC_BITS..CODEC_BITS + 11])
            .fold(0u32, |word, &bit| word << 1 | u32::from(bit));
        let (corrected, _errors) = golay23_correct(received);

        let mut payload = [false; CODEC_BITS];
        payload.copy_from_slice(&self.paired_bits[..CODEC_BITS]);
        for (index, bit) in payload[..8].iter_mut().enumerate() {
            *bit = (corrected >> (22 - index)) & 1 == 1;
        }
        for (index, bit) in payload[11..15].iter_mut().enumerate() {
            *bit = (corrected >> (14 - index)) & 1 == 1;
        }
        payload[2] = payload[1] || payload[3];

        let mut packed = [0u8; 7];
        for (index, &bit) in payload.iter().enumerate() {
            packed[index / 8] |= u8::from(bit) << (7 - index % 8);
        }
        self.vocoder.decode_1300(packed, &mut Speaker(out));
    }
}

const FREEDV_SPOTS: &[HzRange] = &[
    HzRange::spot_khz(7_177.0, 3.0),
    HzRange::spot_khz(14_236.0, 3.0),
    HzRange::spot_khz(21_313.0, 3.0),
    HzRange::spot_khz(28_330.0, 3.0),
];

fn freedv_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "FreeDV",
            &[
                Modulation::Psk4,
                Modulation::Psk2,
                Modulation::Ofdm,
                Modulation::Unknown,
                Modulation::NoiseLike,
            ],
            HzRange::new(900.0, 2_400.0),
            "a digital voice waveform inside an SSB channel",
        )
        .on(FREEDV_SPOTS),
    ]
}

pub(crate) fn freedv_decoder() -> Native<FreeDvParams> {
    Native::new::<FreeDvChannel>(|p| occupied_band(*p))
        .filter(|p| Ok(channel_filter(*p)))
        .signatures(freedv_signatures)
        .form(freedv_form)
        .kinds(freedv_kinds)
}

pub(crate) fn freedv_form() -> Vec<SettingField> {
    vec![
        SettingField::new("sideband", "Side", Control::choice::<Sideband>())
            .title("FreeDV sideband"),
    ]
}

fn freedv_kinds() -> Vec<KindSpec> {
    vec![]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use sdrmm_decoder_test_support::{realtime_budget, settings};
    use sdrmm_wire::ChannelParams;

    #[test]
    fn free_dv_uses_the_selected_sideband() {
        assert_eq!(
            occupied_band(FreeDvParams::default()),
            (LOW_EDGE_HZ, HIGH_EDGE_HZ)
        );
        assert_eq!(
            occupied_band(FreeDvParams {
                sideband: Sideband::Lsb,
                ..FreeDvParams::default()
            }),
            (-HIGH_EDGE_HZ, -LOW_EDGE_HZ)
        );
    }

    #[test]
    fn payload_rebuild_matches_the_free_dv_golay_layout() {
        let data = 0xA53u16;
        let codeword = sdrmm_dsp::golay23_encode(data);
        for damaged in [codeword, codeword ^ 1 << 7, codeword ^ 1 << 1 ^ 1 << 19] {
            let (corrected, errors) = golay23_correct(damaged);
            assert_eq!(corrected >> 11, u32::from(data));
            assert_eq!(errors, (damaged ^ codeword).count_ones());
        }
    }

    #[test]
    fn noise_never_claims_a_freedv_signal() {
        let noise = sdrmm_decoder_test_support::complex_noise(29, 0.3, 8_000 * 60);
        let mut channel = FreeDvChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(
                ChannelParams::new("freedv", &FreeDvParams::default()).expect("freedv settings"),
            ),
        )
        .unwrap();
        let mut out = ChannelOutputs::default();
        for chunk in noise.chunks(1_000) {
            channel.process(chunk, &mut out);
        }
        assert!(out.events.is_empty(), "{:?}", out.events.first());
        assert!(out.audio_pcm.is_empty());
    }

    #[test]
    fn the_frame_that_loses_modem_sync_is_reported_as_rejected() {
        let params = FreeDvParams::default();
        let mut iq = recording();
        iq.extend(std::iter::repeat_n(Complex::new(0.0, 0.0), 32_000));
        let mut channel = FreeDvChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::of(&params).expect("freedv settings")),
        )
        .expect("freedv channel");
        let mut filter =
            sdrmm_decoder_test_support::built_filter(channel_filter(params), INPUT_RATE_HZ);
        let mut filtered = Vec::new();
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(997) {
            filter.process(block, &mut filtered);
            out.reset();
            channel.process(&filtered, &mut out);
        }

        assert!(
            channel.rejected() > 0,
            "the modem frame that lost sync was dropped without a word"
        );
    }

    fn recording() -> Vec<Complex<f32>> {
        const FIXTURE: &[u8] = include_bytes!("../../../fixtures/freedv_1600_8k.sigmf-data");
        FIXTURE
            .as_chunks::<8>()
            .0
            .iter()
            .map(|sample| {
                Complex::new(
                    f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                    f32::from_le_bytes([sample[4], sample[5], sample[6], sample[7]]),
                )
            })
            .collect()
    }

    #[test]
    fn the_scope_sees_four_dqpsk_clusters_in_the_recording() {
        let params = FreeDvParams::default();
        let mut channel = FreeDvChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::of(&params).expect("freedv settings")),
        )
        .expect("freedv channel");
        let mut filter =
            sdrmm_decoder_test_support::built_filter(channel_filter(params), INPUT_RATE_HZ);
        let mut filtered = Vec::new();
        let mut out = ChannelOutputs::default();
        out.symbols.set_wanted(true);
        for block in recording().chunks(8_000) {
            filter.process(block, &mut filtered);
            out.reset();
            channel.process(&filtered, &mut out);
        }
        let tap = &out.symbols;
        assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Carriers));
        assert_eq!(tap.reference.len(), 8);
        assert!(
            tap.symbols.len() >= 2 * 16 * 40,
            "{} values",
            tap.symbols.len()
        );
        assert!(tap.evm < 0.1, "EVM {}", tap.evm);
        assert_eq!(tap.symbol_rate, 800.0);
    }

    #[test]
    fn decodes_the_upstream_receive_recording() {
        const FIXTURE: &[u8] = include_bytes!("../../../fixtures/freedv_1600_8k.sigmf-data");
        let iq: Vec<Complex<f32>> = FIXTURE
            .as_chunks::<8>()
            .0
            .iter()
            .map(|sample| {
                Complex::new(
                    f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                    f32::from_le_bytes([sample[4], sample[5], sample[6], sample[7]]),
                )
            })
            .collect();
        for sideband in [Sideband::Usb, Sideband::Lsb] {
            let params = FreeDvParams {
                sideband,
                ..FreeDvParams::default()
            };
            let mut channel = FreeDvChannel::new(
                ChannelCtx {
                    input_rate: INPUT_RATE_HZ,
                },
                settings(ChannelParams::of(&params).expect("freedv settings")),
            )
            .unwrap();
            let mut filter =
                sdrmm_decoder_test_support::built_filter(channel_filter(params), INPUT_RATE_HZ);
            let mut filtered = Vec::new();
            let mut out = ChannelOutputs::default();
            let mut frames = Vec::new();
            let mut audio = Vec::new();
            let started = Instant::now();
            for block in iq.chunks(997) {
                filter.process(block, &mut filtered);
                out.reset();
                channel.process(&filtered, &mut out);
                frames.append(&mut out.events);
                audio.extend_from_slice(&out.audio_pcm);
            }
            assert!(
                started.elapsed().as_secs_f64() < realtime_budget(2.0),
                "three seconds of FreeDV must decode faster than real time"
            );
            assert!(
                frames.iter().any(
                    |event| event.read::<DvFrame>().is_some_and(
                        |frame| frame.mode == "FreeDV" && frame.kind == DvFrameKind::Header
                    )
                ),
                "FreeDV modem never acquired {sideband:?} sync: {frames:?}"
            );
            assert!(
                audio.iter().any(|sample| sample.abs() > 0.001),
                "Codec2 produced no {sideband:?} speech audio"
            );
        }
    }

    #[test]
    fn noise_never_leaves_the_modem_without_presentable_audio() {
        let params = FreeDvParams::default();
        let mut channel = FreeDvChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::of(&params).expect("freedv settings")),
        )
        .unwrap();
        let mut filter =
            sdrmm_decoder_test_support::built_filter(channel_filter(params), INPUT_RATE_HZ);
        let mut filtered = Vec::new();
        let mut out = ChannelOutputs::default();
        for block in sdrmm_decoder_test_support::complex_noise(97, 1.0, 400_000).chunks(997) {
            filter.process(block, &mut filtered);
            out.reset();
            channel.process(&filtered, &mut out);
            assert!(
                out.audio_pcm.iter().all(|sample| sample.is_finite()),
                "the FreeDV modem produced a non-finite sample from noise"
            );
            assert!(
                out.audio_pcm.iter().all(|sample| sample.abs() <= 1.0),
                "the FreeDV modem produced a sample outside full scale from noise"
            );
        }
    }
}
