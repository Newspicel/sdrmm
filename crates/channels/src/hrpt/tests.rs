use num_complex::Complex;
use sdrmm_decoder_test_support::{complex_noise, settings};
use sdrmm_modem::ccsds;
use sdrmm_wire::ChannelParams;

use super::{
    meteor::{CHANNELS, unpack},
    synth::{level, minor_frame, scan_line, transmission},
    *,
};

const RATE: f64 = INPUT_RATE_HZ;
const BLOCKS: [usize; 4] = [4_096, 1, 65_536, 12_289];

fn channel(satellite: HrptSatellite, picture: HrptPicture) -> HrptChannel {
    let params = HrptParams { satellite, picture };
    HrptChannel::new(
        ChannelCtx {
            input_rate: input_rate(satellite),
        },
        settings(ChannelParams::new("hrpt", &params).expect("hrpt settings")),
    )
    .expect("builds")
}

#[derive(Default)]
struct Received {
    events: Vec<HrptImage>,
    images: Vec<DecodedImage>,
    progress: usize,
}

fn run(chan: &mut HrptChannel, iq: &[Complex<f32>]) -> Received {
    let mut received = Received::default();
    let mut out = ChannelOutputs::default();
    let mut at = 0;
    for &len in BLOCKS.iter().cycle() {
        if at >= iq.len() {
            break;
        }
        let end = (at + len).min(iq.len());
        out.reset();
        chan.process(&iq[at..end], &mut out);
        received.events.extend(
            out.events
                .iter()
                .filter_map(DecoderEvent::read::<HrptImage>),
        );
        received.images.append(&mut out.images);
        received.progress += out.video.len();
        at = end;
    }
    received
}

fn with_quiet(mut iq: Vec<Complex<f32>>, seconds: f64) -> Vec<Complex<f32>> {
    with_quiet_at(&mut iq, seconds, RATE);
    iq
}

fn with_quiet_at(iq: &mut Vec<Complex<f32>>, seconds: f64, rate: f64) {
    iq.extend(complex_noise(3, 0.01, (seconds * rate) as usize));
}

/// Every frame that completes a line, as the demodulator and link recover them from `iq`.
fn lines_of(satellite: HrptSatellite, iq: &[Complex<f32>]) -> Vec<Vec<u8>> {
    let mut demod = SplitPhaseDemod::new(RATE, BIT_RATE, CARRIER_BW_HZ);
    let mut downlink = Downlink::new(satellite).expect("downlink");
    let mut soft = Vec::new();
    let mut lines = Vec::new();
    for chunk in iq.chunks(CHUNK) {
        downlink.feed(chunk, &mut demod, &mut soft);
        let mut at = 0;
        while let Some(pushed) = downlink.next(&soft, &mut at) {
            if pushed.line {
                lines.push(match &downlink {
                    Downlink::Meteor(link) => link.line().to_vec(),
                    Downlink::Noaa(link) => link.frame().to_vec(),
                    Downlink::Metop(link) => link.line().to_vec(),
                });
            }
        }
    }
    lines
}

#[test]
fn meteor_scan_lines_come_back_bit_for_bit() {
    let iq = with_quiet(transmission(HrptSatellite::Meteor, 4, RATE, 21_000.0), 0.01);
    let lines = lines_of(HrptSatellite::Meteor, &iq);
    assert_eq!(lines.len(), 4);
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(line[8..], scan_line(index)[8..], "line {index}");
    }
}

#[test]
fn noaa_minor_frames_come_back_word_for_word() {
    let iq = with_quiet(transmission(HrptSatellite::Noaa, 4, RATE, -31_000.0), 0.01);
    let frames = lines_of(HrptSatellite::Noaa, &iq);
    assert_eq!(frames.len(), 4);
    for (index, frame) in frames.iter().enumerate() {
        let sent = minor_frame(index);
        let wrong: Vec<usize> = (0..noaa::FRAME_WORDS)
            .filter(|&k| noaa::word(frame, k) != sent[k])
            .collect();
        assert!(wrong.is_empty(), "frame {index}: words {wrong:?} differ");
    }
}

#[test]
fn meteor_pixels_unpack_to_the_scene() {
    let line = scan_line(3);
    let mut out = [0; meteor::WIDTH];
    for channel in 0..CHANNELS {
        unpack(&line, channel, &mut out);
        assert!(
            out.iter()
                .enumerate()
                .all(|(x, &value)| value == level(channel, 3, x)),
            "channel {channel}"
        );
    }
}

fn one_pass(satellite: HrptSatellite, width: u16) {
    let rate = input_rate(satellite);
    let mut chan = channel(satellite, HrptPicture::Visible);
    let mut iq = transmission(satellite, 40, rate, -8_000.0);
    with_quiet_at(&mut iq, 2.5, rate);
    let received = run(&mut chan, &iq);
    assert_eq!(received.events.len(), 1);
    let image = &received.events[0];
    assert_eq!(
        (image.satellite, image.lines, image.width),
        (satellite, 40, width)
    );
    assert!(image.complete);
    assert_eq!(image.picture, HrptPicture::Visible);
    assert_eq!(image.first_line.as_deref(), Some("05:00:00 UTC"));
    assert!(
        image.frames_damaged <= 2,
        "{} frames damaged",
        image.frames_damaged
    );
    assert_eq!(received.images.len(), 1);
    let picture = &received.images[0].picture;
    assert_eq!((picture.width, picture.height), (width, 40));
    assert!(received.progress >= 2);
}

#[test]
fn a_meteor_pass_ends_in_one_picture_when_the_signal_fades() {
    one_pass(HrptSatellite::Meteor, 1572);
}

#[test]
fn a_noaa_pass_ends_in_one_picture_when_the_signal_fades() {
    one_pass(HrptSatellite::Noaa, 2048);
}

#[test]
fn a_metop_pass_ends_in_one_picture_when_the_signal_fades() {
    one_pass(HrptSatellite::Metop, 2048);
}

#[test]
fn switching_to_metop_needs_a_new_channel_at_its_rate() {
    let mut chan = channel(HrptSatellite::Meteor, HrptPicture::Auto);
    let metop = HrptParams {
        satellite: HrptSatellite::Metop,
        picture: HrptPicture::Auto,
    };
    assert!(
        chan.apply(settings(
            ChannelParams::new("hrpt", &metop).expect("settings")
        ))
        .is_err()
    );
}

#[test]
fn retuning_closes_the_picture_cut_short() {
    let mut chan = channel(HrptSatellite::Meteor, HrptPicture::Infrared);
    let first = run(
        &mut chan,
        &transmission(HrptSatellite::Meteor, 6, RATE, 0.0),
    );
    assert!(first.events.is_empty());
    chan.retuned();
    let second = run(&mut chan, &[Complex::default(); 16]);
    assert_eq!(second.events.len(), 1);
    assert!(!second.events[0].complete);
    assert_eq!(second.events[0].picture, HrptPicture::Infrared);
}

#[test]
fn switching_satellite_closes_the_picture_under_the_old_one() {
    let mut chan = channel(HrptSatellite::Meteor, HrptPicture::Auto);
    run(
        &mut chan,
        &transmission(HrptSatellite::Meteor, 6, RATE, 0.0),
    );
    let noaa = HrptParams {
        satellite: HrptSatellite::Noaa,
        picture: HrptPicture::Auto,
    };
    chan.apply(settings(
        ChannelParams::new("hrpt", &noaa).expect("settings"),
    ))
    .expect("applies");
    let after = run(
        &mut chan,
        &with_quiet(transmission(HrptSatellite::Noaa, 3, RATE, 0.0), 2.5),
    );
    assert_eq!(after.events.len(), 2);
    assert_eq!(after.events[0].satellite, HrptSatellite::Meteor);
    assert_eq!(
        (after.events[1].satellite, after.events[1].width),
        (HrptSatellite::Noaa, 2048)
    );
}

#[test]
fn noise_alone_draws_nothing() {
    let mut chan = channel(HrptSatellite::Meteor, HrptPicture::Auto);
    let received = run(&mut chan, &complex_noise(9, 0.5, (3.0 * RATE) as usize));
    assert!(received.events.is_empty());
    assert_eq!(received.progress, 0);
}

/// Decodes a raw `cs8` 3 MS/s Meteor HRPT capture named by `HRPT_DEV_CS8`, for tuning only;
/// `HRPT_DEV_CADUS` names a file to write the CADUs to.
#[test]
fn dev_raw_capture() {
    let Ok(path) = std::env::var("HRPT_DEV_CS8") else {
        return;
    };
    let bytes = std::fs::read(path).expect("capture");
    let raw: Vec<Complex<f32>> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[i, q]| Complex::new(f32::from(i as i8) / 128.0, f32::from(q as i8) / 128.0))
        .collect();
    let mut ddc = sdrmm_dsp::Ddc::new(3_000_000.0, RATE, 0.0).expect("ddc");
    let mut iq = Vec::new();
    ddc.process(&raw, &mut iq);
    let mut demod = SplitPhaseDemod::new(RATE, BIT_RATE, CARRIER_BW_HZ);
    let mut link = meteor::Link::new();
    let (mut soft, mut cadus, mut lines, mut damaged) = (Vec::new(), Vec::new(), 0, 0);
    for chunk in iq.chunks(CHUNK) {
        soft.clear();
        demod.process(chunk, &mut soft);
        for &value in &soft {
            let pushed = link.push(value);
            if let Some(framed) = pushed.cadu {
                cadus.extend_from_slice(link.cadu());
                damaged += usize::from(framed.flywheel);
            }
            if pushed.line {
                lines += 1;
                println!("{:?}", meteor::time_ms(link.line()).map(image::time_label));
            }
        }
    }
    println!(
        "cadus {} damaged {damaged} lines {lines}",
        cadus.len() / 1024
    );
    if let Ok(out) = std::env::var("HRPT_DEV_CADUS") {
        std::fs::write(out, &cadus).expect("write cadus");
    }
}

const OFFAIR: &[u8] = include_bytes!("../../../../fixtures/hrpt_meteor_offair_2m66.sigmf-data");

struct OffAir {
    cadus: Vec<u8>,
    damaged: usize,
    lines: Vec<Vec<u8>>,
}

fn decode_offair() -> OffAir {
    let iq = crate::builtin::fixtures::ci16_le(OFFAIR);
    let mut demod = SplitPhaseDemod::new(RATE, BIT_RATE, CARRIER_BW_HZ);
    let mut link = meteor::Link::new();
    let mut soft = Vec::new();
    let mut decoded = OffAir {
        cadus: Vec::new(),
        damaged: 0,
        lines: Vec::new(),
    };
    for chunk in iq.chunks(CHUNK) {
        soft.clear();
        demod.process(chunk, &mut soft);
        for &value in &soft {
            let pushed = link.push(value);
            if let Some(framed) = pushed.cadu {
                decoded.cadus.extend_from_slice(link.cadu());
                decoded.damaged += usize::from(framed.flywheel);
            }
            if pushed.line {
                decoded.lines.push(link.line().to_vec());
            }
        }
    }
    decoded
}

/// The `SatDump` `meteor_hrpt.cadu` for this pass holds the same 60 CADUs, numbers 8889 to 8948.
const SATDUMP_CADUS_CRC32: u32 = 0xD63C_B4AD;

#[test]
fn a_recorded_meteor_pass_decodes_as_satdump_does() {
    let decoded = decode_offair();
    assert_eq!(decoded.cadus.len(), 60 * meteor::CADU_BYTES);
    assert_eq!(decoded.damaged, 0);
    assert_eq!(sdrmm_dsp::crc32_ieee(&decoded.cadus), SATDUMP_CADUS_CRC32);
    let times: Vec<Option<u32>> = decoded
        .lines
        .iter()
        .map(|line| meteor::time_ms(line))
        .collect();
    assert_eq!(
        times,
        [
            Some(19_349_968),
            Some(19_350_141),
            Some(19_350_290),
            Some(19_350_443)
        ]
    );
    assert_eq!(sdrmm_dsp::crc32_ieee(&decoded.lines.concat()), 0x3667_4FB5);
}

/// One of the local-only recordings `reference_data::hrpt_local_dir` names, decoded whole.
fn local_pass(satellite: HrptSatellite, stem: &str) -> Option<Received> {
    let dir = sdrmm_test_support::reference_data::hrpt_local_dir()?;
    let bytes = sdrmm_test_support::reference_data::bytes(&dir, &format!("{stem}.sigmf-data"));
    let iq = with_quiet(sdrmm_decoder_test_support::cf32_le(&bytes), 2.5);
    let received = run(&mut channel(satellite, HrptPicture::Auto), &iq);
    if let (Ok(out), Some(image)) = (std::env::var("HRPT_DEV_PICTURES"), received.images.first()) {
        let path = std::path::Path::new(&out).join(format!("{stem}.rgb"));
        std::fs::write(path, &image.picture.rgb).expect("write picture");
    }
    Some(received)
}

#[test]
fn a_recorded_noaa_pass_draws_its_avhrr_lines() {
    let Some(received) = local_pass(HrptSatellite::Noaa, "noaa_hrpt_2m66") else {
        return;
    };
    let image = &received.events[0];
    assert_eq!((image.lines, image.width), (23, 2048));
    assert_eq!(image.first_line.as_deref(), Some("18:53:39 UTC"));
    assert_eq!(image.picture, HrptPicture::Infrared);
}

#[test]
fn a_second_recorded_meteor_pass_draws_its_msu_mr_lines() {
    let Some(received) = local_pass(HrptSatellite::Meteor, "meteor_hrpt_2m66") else {
        return;
    };
    let image = &received.events[0];
    assert_eq!((image.lines, image.width), (25, 1572));
    assert_eq!(image.first_line.as_deref(), Some("14:16:39 UTC"));
    assert_eq!(image.picture, HrptPicture::Visible);
}

/// 62 CADUs of virtual channel 9 from a `MetOp-B` pass in Meti's sat-archive, as `SatDump` wrote
/// them: derandomized, Reed-Solomon parity in place. They carry AVHRR lines 997 to 1000.
const METOP_CADUS: &[u8] = include_bytes!("../../../../fixtures/hrpt/metop_b_avhrr_cadus.bin");
const METOP_LINE_TIMES: [u32; 4] = [32_816_483, 32_816_650, 32_816_816, 32_816_983];
const METOP_LINES_CRC32: u32 = 0x4B89_458B;

/// The AVHRR packets of `cadus`, read straight off the units with no radio in between.
fn archived_lines(cadus: &[u8]) -> Vec<Vec<u8>> {
    let mut depacketizer = metop::depacketizer();
    let mut lines = Vec::new();
    for cadu in cadus.chunks(ccsds::CADU_BYTES) {
        depacketizer.load(&cadu[ccsds::ASM_BYTES..]);
        while let Some(packet) = depacketizer.next_packet() {
            lines.push(packet.to_vec());
        }
    }
    lines
}

#[test]
fn the_archived_cadus_hold_four_whole_avhrr_lines() {
    let rs = sdrmm_dsp::coding::fec::ccsds_rs::CcsdsReedSolomon::new();
    for cadu in METOP_CADUS.chunks(ccsds::CADU_BYTES) {
        let mut coded = cadu[4..].to_vec();
        let outcome = rs.decode_interleaved(&mut coded, 4);
        assert_eq!((outcome.failed, outcome.corrected), (0, 0));
    }
    let lines = archived_lines(METOP_CADUS);
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|line| metop::parity_holds(line)));
    let times: Vec<Option<u32>> = lines.iter().map(|line| metop::time_ms(line)).collect();
    assert_eq!(times, METOP_LINE_TIMES.map(Some));
    assert_eq!(sdrmm_dsp::crc32_ieee(&lines.concat()), METOP_LINES_CRC32);
}

/// Every AVHRR line the `MetOp` link recovers from `iq`, with the CADUs it counted damaged and
/// all it framed.
fn metop_lines(iq: &[Complex<f32>]) -> (Vec<Vec<u8>>, u32, u32) {
    let mut link = metop::Link::new().expect("link");
    let (mut lines, mut damaged, mut frames) = (Vec::new(), 0, 0);
    for chunk in iq.chunks(CHUNK) {
        link.feed(chunk);
        while let Some(pushed) = link.next() {
            frames += u32::from(pushed.cadu.is_some());
            damaged += u32::from(pushed.cadu.is_some_and(|framed| framed.flywheel));
            if pushed.line {
                lines.push(link.line().to_vec());
            }
        }
    }
    (lines, damaged, frames)
}

/// The filler after the last frame reads as one more frame on position alone, which
/// Reed-Solomon then refuses.
#[test]
fn archived_metop_lines_come_back_through_the_radio() {
    let rate = metop::INPUT_RATE_HZ;
    let mut iq = synth::metop_signal(METOP_CADUS, rate, 23_000.0);
    let noise = complex_noise(4, 0.05, iq.len());
    for (sample, noise) in iq.iter_mut().zip(noise) {
        *sample += noise;
    }
    let (lines, damaged, _) = metop_lines(&iq);
    assert_eq!((damaged, lines.len()), (1, 4));
    assert_eq!(lines, archived_lines(METOP_CADUS));
    with_quiet_at(&mut iq, 2.5, rate);
    let received = run(&mut channel(HrptSatellite::Metop, HrptPicture::Auto), &iq);
    let image = &received.events[0];
    assert_eq!((image.lines, image.width), (4, 2048));
    assert_eq!(image.first_line.as_deref(), Some("09:06:56 UTC"));
    assert_eq!(image.picture, HrptPicture::Visible);
}

#[test]
fn metop_pixels_unpack_to_the_scene() {
    let line = synth::avhrr_packet(3);
    let mut out = [0; metop::WIDTH];
    for channel in 0..5 {
        metop::unpack(&line, channel, &mut out);
        assert!(
            out.iter()
                .enumerate()
                .all(|(x, &value)| value == level(channel, 3, x)),
            "channel {channel}"
        );
    }
    assert!(metop::parity_holds(&line));
}

#[test]
fn metop_noise_alone_draws_nothing() {
    let mut chan = channel(HrptSatellite::Metop, HrptPicture::Auto);
    let noise = complex_noise(9, 0.5, (1.5 * metop::INPUT_RATE_HZ) as usize);
    let received = run(&mut chan, &noise);
    assert!(received.events.is_empty());
    assert_eq!(received.progress, 0);
    assert_eq!(chan.rejected(), 0);
}

/// The local-only off-air `MetOp-B` recording `reference_data::metop_ahrpt_sigid` names. Its
/// carrier jumps a quarter turn every frame or two, so about half its CADUs fail Reed-Solomon
/// and only one AVHRR line comes through whole.
#[test]
fn a_recorded_metop_pass_draws_its_avhrr_lines() {
    let Some(path) = sdrmm_test_support::reference_data::metop_ahrpt_sigid() else {
        return;
    };
    let mut iq =
        sdrmm_decoder_test_support::cf32_le(&sdrmm_test_support::reference_data::read(&path));
    let (lines, damaged, frames) = metop_lines(&iq);
    assert!(frames - damaged > 1_000, "{damaged} of {frames} damaged");
    assert_eq!(lines.len(), 1);
    assert_eq!(metop::time_ms(&lines[0]), Some(METOP_RECORDED_LINE_MS));
    assert_eq!(sdrmm_dsp::crc32_ieee(&lines[0]), METOP_RECORDED_LINE_CRC32);
    with_quiet_at(&mut iq, 2.5, metop::INPUT_RATE_HZ);
    let mut chan = channel(HrptSatellite::Metop, HrptPicture::Auto);
    let received = run(&mut chan, &iq);
    let image = &received.events[0];
    assert_eq!(
        (image.satellite, image.lines, image.width),
        (HrptSatellite::Metop, 1, 2048)
    );
    assert!(chan.rejected() > 0);
}

/// 09:21:01.302 UTC, the one whole line of the recording.
const METOP_RECORDED_LINE_MS: u32 = 33_661_302;
const METOP_RECORDED_LINE_CRC32: u32 = 0xEB67_5E20;

fn scope_of(satellite: HrptSatellite) -> sdrmm_decoder::SymbolTap {
    let iq = transmission(satellite, 4, input_rate(satellite), 3_000.0);
    let mut chan = channel(satellite, HrptPicture::Auto);
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    let mut tap = sdrmm_decoder::SymbolTap::default();
    for block in iq.as_chunks::<CHUNK>().0 {
        out.reset();
        chan.process(block, &mut out);
        if out.symbols.plane.is_some() {
            tap = std::mem::take(&mut out.symbols);
            out.symbols.set_wanted(true);
        }
    }
    tap
}

#[test]
fn the_scope_sees_the_two_split_phase_levels() {
    let tap = scope_of(HrptSatellite::Meteor);
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert!(tap.symbols.len() >= 50, "{} values", tap.symbols.len());
    assert_eq!(tap.symbol_rate, BIT_RATE);
    assert!(
        (tap.freq_error_hz - 3_000.0).abs() < 50.0,
        "{} Hz",
        tap.freq_error_hz
    );
    assert!(tap.evm < 0.05, "EVM {}", tap.evm);
}

#[test]
fn the_scope_sees_four_tight_metop_qpsk_clusters() {
    let tap = scope_of(HrptSatellite::Metop);
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Complex));
    assert_eq!(tap.reference.len(), 8);
    assert!(tap.symbols.len() >= 2 * 50, "{} values", tap.symbols.len());
    assert_eq!(tap.symbol_rate, metop::SYMBOL_RATE);
    assert!(tap.evm < 0.1, "EVM {}", tap.evm);
}
