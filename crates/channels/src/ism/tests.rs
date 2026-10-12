use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass};
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, IsmCoding, IsmIntegrity, IsmMessage,
    IsmModulation, IsmParams,
};

use super::*;
use crate::synth;

macro_rules! fixture {
    ($stem:literal) => {
        include_bytes!(concat!("../../../../fixtures/ism/", $stem, ".sigmf-data")).as_slice()
    };
}

const TAIL: usize = 60_000;
const MEGA: usize = 4;

fn cu8(bytes: &[u8], decimation: usize) -> Vec<Complex<f32>> {
    let iq: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[i, q]| {
            Complex::new(
                (f32::from(i) - 127.5) / 127.5,
                (f32::from(q) - 127.5) / 127.5,
            )
        })
        .collect();
    if decimation == 1 {
        return iq;
    }
    let mut decimator = Decimator::new(&design_lowpass(95, 0.48 / decimation as f64), decimation);
    let mut out = Vec::new();
    decimator.process(&iq, &mut out);
    out
}

fn receiver(unknown: bool) -> IsmChannel {
    let settings = ChannelSettings {
        params: ChannelParams::Ism(IsmParams { unknown }),
        ..ChannelSettings::default_for("ism").unwrap()
    };
    IsmChannel::new(ChannelCtx { input_rate: RATE }, settings).unwrap()
}

fn feed(rx: &mut IsmChannel, iq: &[Complex<f32>]) -> Vec<IsmMessage> {
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(2_048) {
        rx.process(block, &mut out);
    }
    out.events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Ism(message) => Some(message),
            _ => None,
        })
        .collect()
}

fn decode(iq: &[Complex<f32>]) -> (Vec<IsmMessage>, u32) {
    let mut rx = receiver(true);
    let mut padded = iq.to_vec();
    padded.extend(synth::silence(TAIL));
    (feed(&mut rx, &padded), rx.rejected())
}

fn recorded(bytes: &[u8]) -> Vec<IsmMessage> {
    let (messages, rejected) = decode(&cu8(bytes, 1));
    assert_eq!(rejected, 0);
    messages
}

fn only(bytes: &[u8]) -> IsmMessage {
    let mut messages = recorded(bytes);
    assert_eq!(messages.len(), 1, "{messages:?}");
    messages.remove(0)
}

type Reading<'a> = (&'a str, &'a str, Option<f32>, Option<u8>);

fn reading(message: &IsmMessage) -> Reading<'_> {
    let tenths = message.temperature_c.map(|t| (t * 10.0).round() / 10.0);
    (&message.model, &message.id, tenths, message.humidity_pct)
}

#[test]
fn recorded_thermometers_decode_like_rtl_433() {
    let cases: [(&[u8], Reading<'_>); 12] = [
        (
            fixture!("acurite_609txc_433m92_250k"),
            ("Acurite-609TXC", "162", Some(21.8), Some(67)),
        ),
        (
            fixture!("ambient_f007th_433m92_250k"),
            ("Ambientweather-F007TH", "37", Some(19.9), Some(35)),
        ),
        (
            fixture!("ambient_wh31e_915m_250k"),
            ("AmbientWeather-WH31E", "195", Some(12.2), Some(94)),
        ),
        (
            fixture!("fineoffset_wh2_433m92_250k"),
            ("Fineoffset-WH2", "209", Some(24.6), Some(33)),
        ),
        (
            fixture!("lacrosse_tx141_433m92_250k"),
            ("LaCrosse-TX141Bv2", "117", Some(20.1), None),
        ),
        (
            fixture!("nexus_th_433m92_250k"),
            ("Nexus-TH", "181", Some(19.0), Some(71)),
        ),
        (
            fixture!("oregon_thgr122n_433m92_250k"),
            ("Oregon-THGR122N", "248", Some(-5.5), Some(30)),
        ),
        (
            fixture!("oregon_thn132n_433m92_250k"),
            ("Oregon-THN132N", "206", Some(-9.4), None),
        ),
        (
            fixture!("thermopro_tp12_433m92_250k"),
            ("Thermopro-TP12", "56", Some(43.4), None),
        ),
        (
            fixture!("schrader_tpms_315m_250k"),
            ("Schrader", "03A38B2", Some(23.0), None),
        ),
        (
            fixture!("bresser_5in1_868m3_250k"),
            ("Bresser-5in1", "118", Some(8.0), Some(92)),
        ),
        (
            fixture!("fineoffset_wh1080_433m92_250k"),
            ("Fineoffset-WHx080", "5", Some(7.4), Some(82)),
        ),
    ];
    for (bytes, expected) in cases {
        let message = only(bytes);
        assert_eq!(reading(&message), expected);
        assert!(message.snr_db > 6.0, "{message:?}");
    }
}

#[test]
fn two_acurite_towers_in_one_burst_both_report() {
    let messages = recorded(fixture!("acurite_tower_433m92_250k"));
    let readings: Vec<_> = messages.iter().map(reading).collect();
    assert_eq!(
        readings,
        [
            ("Acurite-Tower", "9884", Some(22.8), Some(66)),
            ("Acurite-Tower", "12053", Some(21.0), Some(73)),
        ]
    );
    assert_eq!(messages[0].battery_ok, Some(false));
    assert_eq!(messages[0].channel.as_deref(), Some("B"));
    assert_eq!(messages[1].integrity, IsmIntegrity::Checksum);
}

#[test]
fn lacrosse_fsk_sensors_share_one_burst() {
    let messages = recorded(fixture!("lacrosse_tx29_tx35_868m2_250k"));
    let readings: Vec<_> = messages.iter().map(reading).collect();
    assert_eq!(
        readings,
        [
            ("LaCrosse-TX29IT", "10", Some(23.8), None),
            ("LaCrosse-TX35DTHIT", "26", Some(24.1), Some(34)),
        ]
    );
    assert!(messages.iter().all(|m| m.modulation == IsmModulation::Fsk));
}

#[test]
fn weather_stations_report_wind_and_rain() {
    let five = only(fixture!("acurite_5n1_433m92_250k"));
    assert_eq!(
        (five.model.as_str(), five.id.as_str()),
        ("Acurite-5n1", "839")
    );
    assert_eq!(five.wind_dir_deg, Some(337.5));
    assert!((five.rain_mm.unwrap() - 0.66 * 25.4).abs() < 0.01);
    assert!((five.wind_avg_m_s.unwrap() - 4.311 / 3.6).abs() < 0.01);

    let oregon = only(fixture!("oregon_wgr800_433m92_250k"));
    assert_eq!(
        (oregon.model.as_str(), oregon.wind_max_m_s),
        ("Oregon-WGR800", Some(0.0))
    );

    let bresser = only(fixture!("bresser_5in1_868m3_250k"));
    assert_eq!(
        (bresser.wind_max_m_s, bresser.wind_avg_m_s),
        (Some(3.0), Some(2.0))
    );
    assert_eq!(bresser.wind_dir_deg, Some(292.5));
}

#[test]
fn a_one_megasample_bresser_capture_decodes_after_decimation() {
    let (messages, rejected) = decode(&cu8(fixture!("bresser_6in1_868m3_1000k"), MEGA));
    assert_eq!(rejected, 0);
    let [message] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert_eq!(
        reading(message),
        ("Bresser-6in1", "411042499", Some(11.8), Some(81))
    );
    assert!(
        message
            .wind_max_m_s
            .is_some_and(|max| (max - 5.7).abs() < 0.01)
    );
    assert_eq!(message.wind_dir_deg, Some(276.0));
    assert_eq!(message.integrity, IsmIntegrity::Crc);
}

#[test]
fn remotes_and_door_sensors_report_their_codes() {
    let remote = only(fixture!("generic_remote_433m92_250k"));
    assert_eq!(
        (remote.model.as_str(), remote.id.as_str()),
        ("Generic-Remote", "5069")
    );
    assert_eq!(remote.command, Some(192));
    assert_eq!(remote.tristate.as_deref(), Some("0Z01101Z1000"));

    let door = only(fixture!("honeywell_345m_250k"));
    assert_eq!(
        (door.model.as_str(), door.id.as_str()),
        ("Honeywell-Security", "698779")
    );
    assert_eq!(
        (door.contact_open, door.tamper, door.alarm),
        (Some(true), Some(true), Some(false))
    );

    let tyre = only(fixture!("schrader_tpms_315m_250k"));
    assert_eq!(tyre.pressure_kpa, Some(0.0));
}

#[test]
fn an_unknown_remote_is_shown_only_when_asked() {
    let iq = cu8(fixture!("ev1527_remote_433m92_250k"), 1);
    let unknown = only(fixture!("ev1527_remote_433m92_250k"));
    assert_eq!(unknown.model, sdrmm_wire::UNKNOWN_MODEL);
    assert_eq!((unknown.bits, unknown.coding), (25, IsmCoding::Pwm));
    assert!(unknown.short_us.is_some_and(|us| (330..410).contains(&us)));
    let mut quiet = receiver(false);
    let mut padded = iq;
    padded.extend(synth::silence(TAIL));
    assert!(feed(&mut quiet, &padded).is_empty());
}

#[test]
fn a_weak_recording_in_added_noise_still_decodes() {
    let mut iq = cu8(fixture!("acurite_609txc_433m92_250k"), 1);
    synth::add_noise(&mut iq, 0x15A, 0.05);
    let (messages, _) = decode(&iq);
    assert!(
        messages
            .iter()
            .any(|m| m.model == "Acurite-609TXC" && m.id == "162"),
        "{messages:?}"
    );
}

#[test]
fn noise_alone_reports_nothing() {
    let mut iq = synth::silence(4 * RATE as usize);
    synth::add_noise(&mut iq, 0x5EED, 0.02);
    let (messages, rejected) = decode(&iq);
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(rejected, 0);
}

#[test]
fn repeats_within_two_seconds_are_reported_once() {
    let burst = cu8(fixture!("generic_remote_433m92_250k"), 1);
    let floor = |len: usize, seed: u32| {
        let mut iq = synth::silence(len);
        synth::add_noise(&mut iq, seed, 0.39);
        iq
    };
    let mut iq = burst.clone();
    iq.extend(floor(TAIL, 1));
    iq.extend(&burst);
    iq.extend(floor(3 * RATE as usize, 2));
    iq.extend(&burst);
    iq.extend(floor(TAIL, 3));
    assert_eq!(feed(&mut receiver(true), &iq).len(), 2);
}

#[test]
fn foreign_params_are_refused() {
    let mut settings = ChannelSettings::default_for("ism").unwrap();
    settings.params = ChannelSettings::default_for("eot").unwrap().params;
    assert!(IsmChannel::new(ChannelCtx { input_rate: RATE }, settings.clone()).is_err());
    let mut rx = receiver(true);
    assert!(rx.apply(settings).is_err());
}
