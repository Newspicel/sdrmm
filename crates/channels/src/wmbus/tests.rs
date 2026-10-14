use num_complex::Complex;
use sdrmm_wire::{WmbusFormat, WmbusKey, WmbusMessage, WmbusMode, WmbusQuantity, WmbusSecurity};

use super::*;
use crate::synth::{self, wmbus::WATER_METER};

macro_rules! fixture {
    ($stem:literal) => {
        cf32(include_bytes!(concat!(
            "../../../../fixtures/wmbus/",
            $stem,
            ".sigmf-data"
        )))
    };
}

const TAIL: usize = 4_000;
const KAMSTRUP_ELL: &str =
    "2A442D2C998734761B168D2091D37CAC21E1D68CDAFFCD3DC452BD802913FF7B1706CA9E355D6C2701CC24";
const KAMSTRUP_KEY: &str = "28F64A24988064A079AA2C807D6102AE";

fn cf32(bytes: &[u8]) -> Vec<Complex<f32>> {
    let mut iq: Vec<_> = bytes
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
    iq.extend(synth::silence(TAIL));
    iq
}

fn receiver(params: WmbusParams) -> WmbusChannel {
    let settings = ChannelSettings {
        params: ChannelParams::Wmbus(params),
        ..ChannelSettings::default_for("wmbus").unwrap()
    };
    WmbusChannel::new(ChannelCtx { input_rate: RATE }, settings).unwrap()
}

fn feed(rx: &mut WmbusChannel, iq: &[Complex<f32>]) -> Vec<WmbusMessage> {
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(2_048) {
        rx.process(block, &mut out);
    }
    out.events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Wmbus(message) => Some(message),
            _ => None,
        })
        .collect()
}

fn decode(iq: &[Complex<f32>]) -> Vec<WmbusMessage> {
    let mut rx = receiver(WmbusParams::default());
    let messages = feed(&mut rx, iq);
    assert_eq!(rx.rejected(), 0);
    messages
}

fn only(iq: &[Complex<f32>]) -> WmbusMessage {
    let mut messages = decode(iq);
    assert_eq!(messages.len(), 1, "{messages:?}");
    messages.remove(0)
}

fn bytes(text: &str) -> Vec<u8> {
    crate::crypto::hex(text).unwrap()
}

fn volume(message: &WmbusMessage) -> Option<f64> {
    message
        .records
        .iter()
        .find(|record| record.quantity == WmbusQuantity::Volume && record.storage == 0)
        .and_then(|record| record.value)
}

#[test]
fn a_kamstrup_heat_meter_in_c1_format_b_keeps_its_ell_locked() {
    let message = only(&fixture!("kamstrup_heat_c1b_868m95_400k"));
    assert_eq!(
        (message.mode, message.format),
        (WmbusMode::C, WmbusFormat::B)
    );
    assert_eq!(message.station(), "KAM 60978332");
    assert_eq!(message.device(), "Heat");
    assert_eq!((message.version, message.ci), (0x19, Some(0x8D)));
    assert_eq!(message.access, Some(0xBB));
    assert_eq!(message.security, WmbusSecurity::NoKey);
    assert_eq!(
        message.frame,
        "41442d2c32839760190c8d20bb901f3522d30883bdbfd4eac25b78dcb20a964d8fa3a27b9efe2a38d6a160cc2bdfb310f64faaa672b37d7ad91c9aa244111a78"
    );
}

#[test]
fn a_kamstrup_access_request_in_c1_format_a_carries_only_its_address() {
    let message = only(&fixture!("kamstrup_heat_c1a_868m95_400k"));
    assert_eq!(
        (message.mode, message.format),
        (WmbusMode::C, WmbusFormat::A)
    );
    assert_eq!(message.station(), "KAM 71372984");
    assert_eq!(message.control, 0x47);
    assert_eq!(message.ci, None);
    assert_eq!(message.frame, "09472d2c84293771340c");
}

#[test]
fn a_plain_ell_compact_frame_is_shown_unlocked() {
    let message = only(&fixture!("kamstrup_water_c1b_868m95_400k"));
    assert_eq!(
        (message.mode, message.format),
        (WmbusMode::C, WmbusFormat::B)
    );
    assert_eq!(message.station(), "KAM 74433908");
    assert_eq!(message.device(), "Cold water");
    assert_eq!(message.ci, Some(0x79));
    assert_eq!(message.access, Some(0xC6));
    assert_eq!(message.security, WmbusSecurity::None);
    assert_eq!(
        message.frame,
        "23442d2c083943741b168d20c643aa8905a8727934dd9a810000980f010092fc0000"
    );
}

#[test]
fn two_diehl_t1_frames_decode_back_to_back() {
    let messages = decode(&fixture!("diehl_water_t1_868m95_400k"));
    let stations: Vec<_> = messages.iter().map(WmbusMessage::station).collect();
    assert_eq!(stations, ["DME 84850129", "DME 01820778"]);
    assert_eq!(messages[0].mode, WmbusMode::T);
    assert_eq!(messages[0].device(), "Water");
    assert_eq!(
        messages[0].frame,
        "5344a5112901858476078c00ae900f002c25f00c2f005d8c2c1dac2ca7c07a3a80310710a7f26ca73e8a384744684fe6a79dd0844ebe8c89debb0615906f9f9581b60dbf73e59f525cbc0182172ac76923f254d4"
    );
}

#[test]
fn a_long_header_names_the_meter_and_the_radio_it_came_through() {
    let message = only(&fixture!("imt_repeater_t1_868m95_400k"));
    assert_eq!(message.station(), "IMT 05555487");
    assert_eq!(message.device(), "Water");
    assert_eq!(message.via.as_deref(), Some("IMT 10025571"));
    assert_eq!((message.ci, message.access), (Some(0x72), Some(0x9A)));
    assert_eq!(message.security, WmbusSecurity::NoKey);
}

#[test]
fn a_mode_5_water_meter_without_a_key_stays_locked() {
    let message = only(&fixture!("bmt_water_t1_868m95_400k"));
    assert_eq!(message.station(), "BMT 18160686");
    assert_eq!((message.ci, message.access), (Some(0x7A), Some(0xF0)));
    assert_eq!(message.security, WmbusSecurity::NoKey);
    assert_eq!(
        message.frame,
        "4e44b4098606161813077af000400564157017e38ee57f9b990460cc8244939534d3fa78a08153c58554c8b26f78c995e1e39ad892ede6150123f61a84db7da277f1c0489212e3c26079e16ce024e8"
    );
}

#[test]
fn a_techem_allocator_frame_matches_rtl_433() {
    let message = only(&fixture!("techem_hca_t1_868m95_400k"));
    assert_eq!(message.station(), "TCH 30717777");
    assert_eq!((message.device_type, message.ci), (0x80, Some(0xA0)));
    assert_eq!(
        message.frame,
        "32446850777771306980a011de264401e03406003b0839080600000000051009120d0a1123282718161d0f120a040000000000"
    );
}

#[test]
fn a_techem_allocator_decodes_off_air() {
    let message = only(&fixture!("techem_hca_offair_868m95_400k"));
    assert_eq!(message.station(), "TCH 24186304");
    assert_eq!(
        message.frame,
        "32446850046318246980a0819f330d01d0140000f905ef05000000000000000000000000000000000000000000000000000000"
    );
    assert!(message.snr_db > 6.0, "{}", message.snr_db);
}

pub(crate) fn fixture() -> crate::fixtures::Fixture {
    crate::fixtures::Fixture::new(
        ChannelParams::Wmbus(WmbusParams::default()),
        synthetic(WmbusMode::T, WmbusFormat::A, &WATER_METER),
    )
}

fn synthetic(mode: WmbusMode, format: WmbusFormat, payload: &[u8]) -> Vec<Complex<f32>> {
    let mut iq = synth::wmbus::telegram(mode, format, payload, RATE);
    iq.extend(synth::silence(TAIL));
    iq
}

#[test]
fn every_mode_and_format_reads_plain_records() {
    for (mode, format) in [
        (WmbusMode::S, WmbusFormat::A),
        (WmbusMode::T, WmbusFormat::A),
        (WmbusMode::C, WmbusFormat::A),
        (WmbusMode::C, WmbusFormat::B),
    ] {
        let mut payload = WATER_METER.to_vec();
        if format == WmbusFormat::B {
            payload[0] += 2;
        }
        let message = only(&synthetic(mode, format, &payload));
        assert_eq!((message.mode, message.format), (mode, format));
        assert_eq!(message.station(), "SEN 33225544");
        assert_eq!(message.security, WmbusSecurity::None);
        assert_eq!(volume(&message), Some(123.529));
        assert_eq!(message.summary(), "Water SEN 33225544 · Volume 123.529 m³");
    }
}

#[test]
fn a_detuned_noisy_telegram_still_decodes() {
    for offset_hz in [-40_000.0, 40_000.0] {
        let mut iq = synthetic(WmbusMode::T, WmbusFormat::A, &WATER_METER);
        synth::shift(&mut iq, offset_hz, RATE);
        synth::add_noise(&mut iq, 7, 0.35);
        let message = only(&iq);
        assert_eq!(volume(&message), Some(123.529));
    }
}

#[test]
fn three_of_six_soft_decisions_repair_a_faded_chip() {
    let chips = synth::wmbus::chips(WmbusMode::T, WmbusFormat::A, &WATER_METER);
    let faded = 38 + 10 + 12 * 20 + 3;
    let per_chip = (RATE / 100_000.0) as usize;
    let levels: Vec<f32> = chips
        .iter()
        .enumerate()
        .flat_map(|(index, &chip)| {
            let level = if chip { 1.0 } else { -1.0 };
            let level = if index == faded { -0.5 * level } else { level };
            std::iter::repeat_n(level, per_chip)
        })
        .collect();
    let mut iq = synth::fm_modulate(&levels, 50_000.0, RATE);
    iq.extend(synth::silence(TAIL));
    let message = only(&iq);
    assert_eq!(message.corrected, 1);
    assert_eq!(volume(&message), Some(123.529));
}

#[test]
fn a_frame_failing_its_crc_is_counted_not_shown() {
    let mut chips = synth::wmbus::chips(WmbusMode::C, WmbusFormat::A, &WATER_METER);
    let flip = 32 + 32 + 8 * 20;
    chips[flip] = !chips[flip];
    let mut iq = synth::fsk(&chips, 100_000.0, 50_000.0, RATE);
    iq.extend(synth::silence(TAIL));
    let mut rx = receiver(WmbusParams::default());
    assert!(feed(&mut rx, &iq).is_empty());
    assert_eq!(rx.rejected(), 1);
}

#[test]
fn a_key_added_later_opens_the_meter() {
    let iq = synthetic(WmbusMode::C, WmbusFormat::A, &bytes(KAMSTRUP_ELL));
    let mut rx = receiver(WmbusParams::default());
    let locked = feed(&mut rx, &iq);
    assert_eq!(locked[0].security, WmbusSecurity::NoKey);
    let keys = WmbusParams {
        keys: vec![WmbusKey {
            meter: "76348799".to_owned(),
            key: KAMSTRUP_KEY.to_owned(),
        }],
    };
    rx.apply(ChannelSettings {
        params: ChannelParams::Wmbus(keys),
        ..ChannelSettings::default_for("wmbus").unwrap()
    })
    .unwrap();
    let opened = feed(&mut rx, &iq);
    assert_eq!(opened[0].security, WmbusSecurity::Decrypted);
    assert_eq!(volume(&opened[0]), Some(6.408));
    assert_eq!(
        opened[0].summary(),
        "Cold water KAM 76348799 · Volume 6.408 m³"
    );
}

#[test]
fn a_malformed_key_is_refused() {
    let settings = ChannelSettings {
        params: ChannelParams::Wmbus(WmbusParams {
            keys: vec![WmbusKey {
                meter: "76348799".to_owned(),
                key: "28F6".to_owned(),
            }],
        }),
        ..ChannelSettings::default_for("wmbus").unwrap()
    };
    assert!(WmbusChannel::new(ChannelCtx { input_rate: RATE }, settings).is_err());
}

#[test]
fn noise_alone_decodes_nothing() {
    let mut iq = synth::silence(400_000);
    synth::add_noise(&mut iq, 11, 0.5);
    let mut rx = receiver(WmbusParams::default());
    assert!(feed(&mut rx, &iq).is_empty());
}
