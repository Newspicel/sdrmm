use sdrmm_wire::FilterSpec;
use std::sync::LazyLock;

use crate::dvbt::DvbtParams;
use num_complex::Complex;
use sdrmm_decoder::broadcast::{
    BroadcastService, BroadcastServiceKind, BroadcastStatus, publish_status,
};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, ClickProfile, DecoderFamily, EventContract};

use super::frontend::Frontend;
use sdrmm_decoder::{Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_rate};
use sdrmm_media::{Codec, Media};
use sdrmm_modem::{
    dvb::PACKET,
    mpegts::{PesUnit, StreamKind, TsDemux},
};

pub const INPUT_RATE: f64 = 64_000_000.0 / 7.0;
const TAPS_PER_SECOND: f64 = 10.0;
static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dvbt".to_owned(),
    name: "DVB-T/T2".to_owned(),
    summary: "DVB-T and T2 digital TV".to_owned(),
    family: DecoderFamily::BROADCAST,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 8_000_000.0,
    input_rate_hz: INPUT_RATE,
    has_audio: true,
    has_video: true,
    emits: vec![BroadcastStatus::KIND],
    click: ClickProfile::Vocoder,
    ..ChannelDescriptor::default()
});

pub fn occupied_band(params: &DvbtParams) -> (f64, f64) {
    let half = params.bandwidth.hz() / 2.0;
    (-half, half)
}

pub fn channel_filter(params: &DvbtParams) -> FilterSpec {
    FilterSpec::lowpass(127, params.bandwidth.hz() / 2.0)
}

fn params(settings: &ChannelSettings) -> Result<DvbtParams, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
}

#[derive(PartialEq, Eq)]
struct Selection {
    program: u16,
    audio: Option<(u16, StreamKind)>,
    video: Option<(u16, StreamKind)>,
}

pub struct DvbtChannel {
    params: DvbtParams,
    receiver: Frontend,
    packets: Vec<[u8; PACKET]>,
    units: Vec<PesUnit>,
    demux: TsDemux,
    media: Media,
    selection: Option<Selection>,
    report_samples: usize,
    tap_samples: usize,
    was_locked: bool,
}

impl DvbtChannel {
    fn play(&mut self, out: &mut ChannelOutputs) {
        let selected = self.demux.program().map(|program| Selection {
            program: program.number,
            audio: program
                .streams
                .iter()
                .find(|s| s.kind.is_audio())
                .map(|s| (s.pid, s.kind)),
            video: program
                .streams
                .iter()
                .find(|s| s.kind.is_video())
                .map(|s| (s.pid, s.kind)),
        });
        if selected != self.selection {
            self.media.reset();
            self.selection = selected;
        }
        if let Some(selection) = &self.selection {
            for unit in &self.units {
                for (pid, kind) in [selection.audio, selection.video].into_iter().flatten() {
                    if unit.pid == pid
                        && let Some(codec) = Codec::of_stream(kind)
                    {
                        self.media.push(&codec, &unit.payload, unit.pts);
                    }
                }
            }
        }
        self.media.drain(out);
    }

    /// Shows one symbol's cells ten times a second, enough to read the QAM without flooding
    /// the scope.
    fn tap_symbols(&mut self, samples: usize, out: &mut ChannelOutputs) {
        if !out.symbols.wanted() {
            return;
        }
        let rate = self.params.sample_rate_hz();
        self.receiver.tap(&mut out.symbols, rate);
        if self.tap_samples == 0 {
            self.receiver.request_tap();
        }
        self.tap_samples += samples;
        if self.tap_samples as f64 >= rate / TAPS_PER_SECOND {
            self.tap_samples = 0;
        }
    }

    fn report(&self, out: &mut ChannelOutputs) {
        let program = self.demux.program();
        let mut status = self.receiver.status(self.params.sample_rate_hz());
        let selected = program.map(|p| u32::from(p.number));
        let services = self
            .demux
            .programs()
            .map(|p| BroadcastService {
                id: u32::from(p.number),
                label: p
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Program {}", p.number)),
                kind: if p.streams.iter().any(|s| s.kind.is_video()) {
                    BroadcastServiceKind::Video
                } else if p.streams.iter().any(|s| s.kind.is_audio()) {
                    BroadcastServiceKind::Audio
                } else {
                    BroadcastServiceKind::Data
                },
                selected: selected == Some(u32::from(p.number)),
                bitrate_kbps: None,
                language: p.streams.iter().find_map(|s| s.language.clone()),
            })
            .collect();
        status.service_id = selected;
        status.label = program.and_then(|p| p.name.clone());
        status.ensemble_label = program.and_then(|p| p.provider.clone());
        status.audio_frames_ok = self.media.audio_frames;
        status.audio_frames_bad = self.media.audio_errors;
        status.audio_error.clone_from(&self.media.audio_error);
        status.video_frames_ok = self.media.video_frames;
        status.video_frames_bad = self.media.video_errors;
        status.video_error.clone_from(&self.media.video_error);
        status.services = services;
        publish_status(out, &status, "program");
    }
}

impl NativeChannel for DvbtChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, params.sample_rate_hz())?;
        let mut demux = TsDemux::new();
        demux.select(params.program);
        let mut media = Media::new()?;
        media.enable_clock();
        Ok(Self {
            params,
            receiver: Frontend::new(params)?,
            packets: Vec::with_capacity(8192),
            units: Vec::with_capacity(32),
            demux,
            media,
            selection: None,
            report_samples: 0,
            tap_samples: 0,
            was_locked: false,
        })
    }
}

impl Channel for DvbtChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let wanted = params(&settings)?;
        if wanted.standard != self.params.standard
            || wanted.plp != self.params.plp
            || wanted.bandwidth != self.params.bandwidth
            || wanted.low_priority != self.params.low_priority
        {
            self.retuned();
        }
        self.receiver.apply(wanted)?;
        self.demux.select(wanted.program);
        self.params = wanted;
        Ok(())
    }
    fn retuned(&mut self) {
        self.receiver.reset();
        self.demux.reset();
        self.media.reset();
        self.selection = None;
        self.report_samples = 0;
        self.tap_samples = 0;
        self.was_locked = false;
    }
    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let input_rate = self.params.sample_rate_hz();
        self.media.advance(iq.len(), input_rate);
        self.packets.clear();
        self.receiver.push(iq, &mut self.packets);
        self.tap_symbols(iq.len(), out);
        let locked = self.receiver.locked();
        if self.was_locked && !locked {
            self.demux.reset();
            self.media.reset();
            self.selection = None;
        }
        self.was_locked = locked;
        self.units.clear();
        for packet in &self.packets {
            self.demux.push(packet, &mut self.units);
        }
        self.play(out);
        self.report_samples += iq.len();
        if self.report_samples >= (input_rate / 4.0) as usize {
            self.report_samples %= (input_rate / 4.0) as usize;
            self.report(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_decoder::broadcast::latest_status;
    use sdrmm_wire::ChannelParams;

    fn settings(bandwidth: crate::dvbt::DvbtBandwidth) -> ChannelSettings {
        ChannelSettings {
            frequency_hz: 0.0,
            squelch: sdrmm_wire::Squelch::Off,
            params: ChannelParams::new(
                "dvbt",
                &DvbtParams {
                    bandwidth,
                    ..Default::default()
                },
            )
            .expect("dvbt settings"),
            blanker: Default::default(),
            if_filter: Default::default(),
        }
    }

    #[test]
    fn narrow_channels_decode_from_a_two_and_a_half_megasample_radio() {
        for bandwidth in [
            crate::dvbt::DvbtBandwidth::Khz250,
            crate::dvbt::DvbtBandwidth::Khz333,
            crate::dvbt::DvbtBandwidth::Khz500,
            crate::dvbt::DvbtBandwidth::Mhz1,
            crate::dvbt::DvbtBandwidth::Mhz1_7,
            crate::dvbt::DvbtBandwidth::Mhz2,
        ] {
            decodes_through_radio_chain(bandwidth, 2_500_000.0);
        }
    }

    fn decodes_through_radio_chain(bandwidth: crate::dvbt::DvbtBandwidth, radio_rate: f64) {
        let channel_settings = settings(bandwidth);
        let input_rate = sdrmm_decoder::input_rate(&channel_settings.params);
        assert_eq!(input_rate, bandwidth.sample_rate_hz());
        let half = bandwidth.hz() / 2.0;
        assert_eq!(
            sdrmm_decoder::occupied_band(&channel_settings.params),
            (-half, half)
        );
        let mut native = crate::synth::dvbt::waveform(crate::synth::dvbt::defaults(), 180);
        sdrmm_modem::synth::shift(&mut native, 600.0, input_rate);
        let iq = sdrmm_modem::synth::resample(&native, input_rate, radio_rate);
        let mut ddc = sdrmm_dsp::Ddc::new(radio_rate, input_rate, 0.0).unwrap();
        let mut filter = sdrmm_decoder::channel_filter(&channel_settings.params).unwrap();
        let mut receiver = DvbtChannel::new(ChannelCtx { input_rate }, channel_settings).unwrap();
        let mut baseband = Vec::new();
        let mut filtered = Vec::new();
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(1009) {
            ddc.process(block, &mut baseband);
            filter.process(&baseband, &mut filtered);
            out.reset();
            receiver.process(&filtered, &mut out);
        }
        assert!(receiver.receiver.locked(), "{bandwidth:?}");
        assert_eq!(
            receiver.demux.program().and_then(|p| p.name.as_deref()),
            Some("Rust TV"),
            "{bandwidth:?}"
        );
        out.reset();
        receiver.report(&mut out);
        let status = latest_status(&out).expect("broadcast status");
        assert!(
            (status.frequency_error_hz - 600.0).abs() < 30.0,
            "{bandwidth:?} {}",
            status.frequency_error_hz
        );
        assert!(
            DvbtChannel::new(
                ChannelCtx {
                    input_rate: INPUT_RATE
                },
                settings(bandwidth)
            )
            .is_err()
        );
    }

    #[test]
    fn the_scope_sees_one_symbol_of_tight_16_qam() {
        let iq = crate::synth::dvbt::waveform(crate::synth::dvbt::defaults(), 180);
        let mut channel = DvbtChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE,
            },
            settings(crate::dvbt::DvbtBandwidth::Mhz8),
        )
        .expect("a DVB-T channel");
        let mut out = ChannelOutputs::default();
        out.symbols.set_wanted(true);
        for block in iq.chunks(16_384) {
            channel.process(block, &mut out);
        }
        let tap = &out.symbols;
        assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Carriers));
        assert_eq!(tap.reference.len(), 2 * 16);
        assert_eq!(tap.symbols.len(), 2 * 1512);
        assert!(tap.evm < 0.05, "EVM {}", tap.evm);
        assert!((tap.symbol_rate - 1512.0 * INPUT_RATE / 2304.0).abs() < 1.0);
    }

    fn t2_scope(bytes: &[u8]) -> sdrmm_decoder::SymbolTap {
        let params = DvbtParams {
            standard: crate::dvbt::DvbtStandard::DvbT2,
            bandwidth: crate::dvbt::DvbtBandwidth::Mhz1_7,
            plp: None,
            ..Default::default()
        };
        let mut config = settings(params.bandwidth);
        config.params = ChannelParams::new("dvbt", &params).expect("dvbt settings");
        let rate = sdrmm_decoder::input_rate(&config.params);
        let iq: Vec<_> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&[a, b, c, d, e, f, g, h]| {
                Complex::new(
                    f32::from_le_bytes([a, b, c, d]),
                    f32::from_le_bytes([e, f, g, h]),
                )
            })
            .collect();
        let mut channel =
            DvbtChannel::new(ChannelCtx { input_rate: rate }, config).expect("a T2 channel");
        let mut out = ChannelOutputs::default();
        out.symbols.set_wanted(true);
        for block in iq.chunks(1009) {
            channel.process(block, &mut out);
        }
        out.symbols
    }

    #[test]
    fn the_t2_scope_sees_the_plp_grid_with_rotation_undone() {
        for (bytes, points) in [
            (
                include_bytes!("../../../../fixtures/dvbt2/rf_2k_qpsk.f32").as_slice(),
                4,
            ),
            (
                include_bytes!("../../../../fixtures/dvbt2/rf_2k_lite.f32").as_slice(),
                16,
            ),
        ] {
            let tap = t2_scope(bytes);
            assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Carriers));
            assert_eq!(tap.reference.len(), 2 * points);
            assert!(!tap.symbols.is_empty());
            assert!(tap.evm < 0.05, "{points} points: EVM {}", tap.evm);
            assert!(tap.symbol_rate > 1.0e6, "{}", tap.symbol_rate);
        }
    }

    #[test]
    fn changing_bandwidth_discards_decoder_and_media_state() {
        let bandwidth = crate::dvbt::DvbtBandwidth::Mhz1_7;
        let mut receiver = DvbtChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE,
            },
            settings(crate::dvbt::DvbtBandwidth::Mhz8),
        )
        .unwrap();
        let iq = crate::synth::dvbt::waveform(crate::synth::dvbt::defaults(), 180);
        let mut out = ChannelOutputs::default();
        receiver.process(&iq, &mut out);
        assert!(receiver.receiver.locked());
        receiver.apply(settings(bandwidth)).unwrap();
        assert!(!receiver.receiver.locked());
        assert!(receiver.selection.is_none());
        assert_eq!(receiver.report_samples, 0);
        out.reset();
        receiver.process(&iq, &mut out);
        assert!(receiver.receiver.locked());
    }

    #[test]
    fn every_bandwidth_discovers_services_at_its_native_clock() {
        let iq = crate::synth::dvbt::waveform(crate::synth::dvbt::defaults(), 180);
        for bandwidth in [
            crate::dvbt::DvbtBandwidth::Mhz1_7,
            crate::dvbt::DvbtBandwidth::Mhz6,
            crate::dvbt::DvbtBandwidth::Mhz7,
            crate::dvbt::DvbtBandwidth::Mhz8,
        ] {
            let params = DvbtParams {
                bandwidth,
                ..Default::default()
            };
            let mut channel = DvbtChannel::new(
                ChannelCtx {
                    input_rate: bandwidth.sample_rate_hz(),
                },
                ChannelSettings {
                    frequency_hz: 0.0,
                    squelch: sdrmm_wire::Squelch::Off,
                    params: ChannelParams::new("dvbt", &params).expect("dvbt settings"),
                    blanker: Default::default(),
                    if_filter: Default::default(),
                },
            )
            .unwrap();
            let mut out = ChannelOutputs::default();
            for block in iq.chunks(16384) {
                out.reset();
                channel.process(block, &mut out);
            }
            assert!(channel.receiver.locked(), "{bandwidth:?}");
            assert_eq!(
                channel.demux.program().and_then(|p| p.name.as_deref()),
                Some("Rust TV")
            );
        }
    }
    #[test]
    fn narrow_t2_channel_recovers_transport_from_a_two_megasample_radio() {
        let params = DvbtParams {
            standard: crate::dvbt::DvbtStandard::DvbT2,
            bandwidth: crate::dvbt::DvbtBandwidth::Mhz1_7,
            plp: Some(7),
            ..Default::default()
        };
        let mut config = settings(params.bandwidth);
        config.params = ChannelParams::new("dvbt", &params).expect("dvbt settings");
        let rate = sdrmm_decoder::input_rate(&config.params);
        assert_eq!(rate, 131_000_000.0 / 71.0);
        let bytes = include_bytes!("../../../../fixtures/dvbt2/rf_2k_qpsk.f32");
        let native: Vec<_> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&[a, b, c, d, e, f, g, h]| {
                Complex::new(
                    f32::from_le_bytes([a, b, c, d]),
                    f32::from_le_bytes([e, f, g, h]),
                )
            })
            .collect();
        let iq = sdrmm_modem::synth::resample(&native, rate, 2_048_000.0);
        let mut ddc = sdrmm_dsp::Ddc::new(2_048_000.0, rate, 0.0).unwrap();
        let mut filter = sdrmm_decoder_test_support::built_filter(channel_filter(&params), rate);
        let mut channel =
            DvbtChannel::new(ChannelCtx { input_rate: rate }, config.clone()).unwrap();
        let mut baseband = Vec::with_capacity(8192);
        let mut filtered = Vec::with_capacity(8192);
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(1009) {
            baseband.clear();
            filtered.clear();
            ddc.process(block, &mut baseband);
            filter.process(&baseband, &mut filtered);
            channel.process(&filtered, &mut out);
        }
        channel.report(&mut out);
        let status = latest_status(&out).unwrap();
        assert_eq!(status.system, "dvb_t2");
        assert_eq!(status.frames_ok, 4, "{status:?}");
        assert_eq!(status.frames_bad, 0, "{status:?}");
        let mut changed = params;
        changed.plp = Some(9);
        config.params = ChannelParams::new("dvbt", &changed).expect("dvbt settings");
        channel.apply(config).unwrap();
        assert!(!channel.receiver.locked());
    }

    #[test]
    fn t2_iq_discovers_program_and_delivers_audio_and_video() {
        let params = DvbtParams {
            standard: crate::dvbt::DvbtStandard::DvbT2,
            ..Default::default()
        };
        let mut config = settings(params.bandwidth);
        config.params = ChannelParams::new("dvbt", &params).expect("dvbt settings");
        let rate = params.sample_rate_hz();
        let bytes = include_bytes!("../../../../fixtures/dvbt2/rf_32k_media.f32");
        let iq: Vec<_> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&[a, b, c, d, e, f, g, h]| {
                Complex::new(
                    f32::from_le_bytes([a, b, c, d]),
                    f32::from_le_bytes([e, f, g, h]),
                )
            })
            .collect();
        let mut channel = DvbtChannel::new(ChannelCtx { input_rate: rate }, config).unwrap();
        let mut out = ChannelOutputs::default();
        for block in iq[..iq.len() - 8192 + 32].chunks(1009) {
            channel.process(block, &mut out);
        }
        assert_eq!(channel.demux.program().map(|p| p.number), Some(1));
        assert!(channel.receiver.locked());
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            channel.media.advance((rate / 50.0) as usize, rate);
            channel.media.drain(&mut out);
            if channel.media.audio_frames > 0 && channel.media.video_frames > 0 {
                break;
            }
        }
        assert_eq!(
            channel.media.audio_errors, 0,
            "{:?}",
            channel.media.audio_error
        );
        assert_eq!(
            channel.media.video_errors, 0,
            "{:?}",
            channel.media.video_error
        );
        assert!(channel.media.audio_frames > 0);
        assert!(channel.media.video_frames > 0);
        assert!(out.audio_pcm.iter().any(|&sample| sample.abs() > 0.01));
    }

    fn t2_frames(bytes: &[u8], paths: &[(usize, f32, f32)], snr_db: f32) -> (u32, u32) {
        let native = cells(bytes);
        let power = native
            .iter()
            .map(num_complex::Complex::norm_sqr)
            .sum::<f32>()
            / native.len() as f32;
        let clean: Vec<_> = native.iter().chain(&native).copied().collect();
        let mut iq = clean.clone();
        for (i, sample) in iq.iter_mut().enumerate() {
            for &(delay, re, im) in paths {
                if i >= delay {
                    *sample += clean[i - delay] * Complex::new(re, im);
                }
            }
        }
        sdrmm_modem::synth::add_noise(
            &mut iq,
            0x77,
            (power / 10f32.powf(snr_db / 10.0) * 1.5).sqrt(),
        );
        t2_status(&iq)
    }

    fn cells(bytes: &[u8]) -> Vec<Complex<f32>> {
        bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&[a, b, c, d, e, f, g, h]| {
                Complex::new(
                    f32::from_le_bytes([a, b, c, d]),
                    f32::from_le_bytes([e, f, g, h]),
                )
            })
            .collect()
    }

    fn t2_status(iq: &[Complex<f32>]) -> (u32, u32) {
        let params = DvbtParams {
            standard: crate::dvbt::DvbtStandard::DvbT2,
            bandwidth: crate::dvbt::DvbtBandwidth::Mhz1_7,
            plp: None,
            ..Default::default()
        };
        let mut config = settings(params.bandwidth);
        config.params = ChannelParams::new("dvbt", &params).expect("dvbt settings");
        let rate = sdrmm_decoder::input_rate(&config.params);
        let mut channel = DvbtChannel::new(ChannelCtx { input_rate: rate }, config).unwrap();
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(1009) {
            channel.process(block, &mut out);
        }
        channel.report(&mut out);
        latest_status(&out).map_or((0, 0), |status| (status.frames_ok, status.frames_bad))
    }

    /// The 2K QPSK fixture's frames played back to back, `frames` long, as a broadcast sends them.
    fn t2_broadcast(frames: usize) -> Vec<Complex<f32>> {
        let fixture = cells(include_bytes!("../../../../fixtures/dvbt2/rf_2k_qpsk.f32"));
        let pair = &fixture[73..73 + 2 * T2_FRAME];
        let mut iq = vec![Complex::default(); 73];
        iq.extend(pair.iter().cycle().take(frames * T2_FRAME));
        iq.extend(std::iter::repeat_n(Complex::default(), 8192));
        iq
    }

    const T2_FRAME: usize = 63_488;

    #[test]
    fn t2_follows_a_receiver_clock_far_off_the_transmitters() {
        use sdrmm_modem_test_support::ber::{
            impair::{ClockError, Impairment},
            rng::Rng,
        };
        let mut iq = t2_broadcast(16);
        ClockError::new(200.0).apply(&mut iq, &mut Rng::new(1));
        assert_eq!(t2_status(&iq), (32, 0));
    }

    #[test]
    fn t2_learns_how_far_the_receiver_clock_runs() {
        use sdrmm_modem_test_support::ber::{
            impair::{ClockError, Impairment},
            rng::Rng,
        };
        let mut iq = t2_broadcast(16);
        ClockError::new(200.0).apply(&mut iq, &mut Rng::new(1));
        let mut receiver = crate::dvbt::t2::receiver::Receiver::new(None).unwrap();
        let mut packets = Vec::with_capacity(4096);
        for chunk in iq.chunks(997) {
            receiver.push(chunk, &mut packets);
        }
        let ppm = receiver.clock_error() * 1e6;
        assert!((ppm - 200.0).abs() < 20.0, "learned {ppm:.1} ppm");
    }

    #[test]
    fn t2_counts_a_frame_whose_p1_never_came() {
        let mut iq = t2_broadcast(8);
        let dropped = 73 + 3 * T2_FRAME;
        iq[dropped..dropped + T2_FRAME].fill(Complex::default());
        assert_eq!(t2_status(&iq), (14, 1));
    }

    #[test]
    fn t2_decodes_near_its_code_threshold_and_through_long_echoes() {
        let qpsk = include_bytes!("../../../../fixtures/dvbt2/rf_2k_qpsk.f32").as_slice();
        let lite = include_bytes!("../../../../fixtures/dvbt2/rf_2k_lite.f32").as_slice();
        for (name, bytes, paths, snr_db) in [
            ("QPSK in noise", qpsk, &[][..], 6.0),
            (
                "QPSK past the 3-carrier pilot reach",
                qpsk,
                &[(300, 0.0, 0.5)][..],
                6.0,
            ),
            (
                "QPSK under a strong long echo",
                qpsk,
                &[(300, 0.0, 0.8)][..],
                12.0,
            ),
            (
                "rotated 16-QAM in a deep notch channel",
                lite,
                &[(7, -0.9, 0.0)][..],
                12.0,
            ),
        ] {
            let (ok, bad) = t2_frames(bytes, paths, snr_db);
            assert!(
                ok >= 4 && ok / 2 + bad >= 3,
                "{name} at {snr_db} dB: {ok} good packets, {bad} bad frames"
            );
        }
    }
}
