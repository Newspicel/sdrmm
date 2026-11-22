use super::*;
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::present::{EventPayload, FieldValue, Unit, number};

/// horusdemodlib decodes its audio samples with the tone search above 0 Hz, which keeps the
/// mirror image of a real signal out.
const AUDIO_SEARCH_HZ: (f32, f32) = (100.0, 3_900.0);

const V1_RAW: &str = "0112000000230000000000000000000000001C9A9545";
const V2_RAW: &str = "000108000C223800000000000000000000000000000102030405060708091DBB";
const V3_RAW: &str = "5AC0401A1917967D079F021100000000889545744AA2881083CEB9ECC2B9ECC2802F79D73D98573D985005EF3AE7B30AE7B30A00BDE75CF6615CF6614017B20574F4F46574F4F46574F4F46574F4F46574F4F46574F4F46574F4F46574F4F460E02ED8FF0100000000000042000000000C6AE5910100000040416F0002000000";

fn heard(bytes: &[u8], search_hz: (f32, f32)) -> Vec<HorusFrame> {
    let mut receiver = HorusReceiver::new(search_hz).expect("receiver");
    let mut frames = Vec::new();
    for block in cf32_le(bytes).chunks(1_000) {
        receiver.process(block, &mut frames);
    }
    frames
}

#[test]
fn decodes_the_noisy_v1_reference() {
    let frames = heard(
        include_bytes!("../../../../fixtures/horus/horus_v1_4db5_8k.sigmf-data"),
        AUDIO_SEARCH_HZ,
    );
    let [frame] = frames.as_slice() else {
        panic!("one frame, got {frames:?}");
    };
    assert_eq!(frame.raw, V1_RAW);
    assert_eq!(frame.version, 1);
    assert_eq!(frame.payload, "1");
    assert_eq!(frame.sequence, 18);
    assert_eq!(frame.time.as_deref(), Some("00:00:35"));
    assert_eq!(frame.temperature_c, Some(28.0));
    assert!(frame.errors_corrected > 0, "4.5 dB should need Golay");
}

#[test]
fn decodes_the_v2_reference() {
    let frames = heard(
        include_bytes!("../../../../fixtures/horus/horus_v2_8k.sigmf-data"),
        AUDIO_SEARCH_HZ,
    );
    let [frame] = frames.as_slice() else {
        panic!("one frame, got {frames:?}");
    };
    assert_eq!(frame.raw, V2_RAW);
    assert_eq!(frame.version, 2);
    assert_eq!(frame.payload, "256");
    assert_eq!(frame.sequence, 8);
    assert_eq!(frame.time.as_deref(), Some("12:34:56"));
    assert_eq!(frame.custom.as_deref(), Some("010203040506070809"));
}

#[test]
fn decodes_the_v3_reference() {
    let frames = heard(
        include_bytes!("../../../../fixtures/horus/horus_v3_8k.sigmf-data"),
        AUDIO_SEARCH_HZ,
    );
    let [frame] = frames.as_slice() else {
        panic!("one frame, got {frames:?}");
    };
    assert_eq!(frame.raw, V3_RAW);
    assert_eq!(frame.version, 3);
    assert_eq!(frame.sequence, 0);
    assert_eq!(frame.payload, "4FSKTEST-V2");
    assert_eq!(frame.time.as_deref(), Some("00:00:03"));
    assert_eq!((frame.lat, frame.lon), (Some(0.00023), Some(0.00034)));
    assert_eq!(frame.altitude_m, Some(56.0));
    assert_eq!(frame.sensors.len(), 4);
    assert!(
        frame
            .sensors
            .iter()
            .all(|sensor| sensor.name == "meowmeow" && sensor.value == "123")
    );
    assert_eq!(frame.custom.as_deref(), Some(&"574F4F46".repeat(8)[..]));
}

#[test]
fn the_scope_sees_four_tight_tone_levels() {
    let mut receiver = HorusReceiver::new(AUDIO_SEARCH_HZ).expect("receiver");
    receiver.watch(true);
    let mut frames = Vec::new();
    let iq = cf32_le(include_bytes!(
        "../../../../fixtures/horus/horus_v2_8k.sigmf-data"
    ));
    let (mut seen, mut worst) = (0, 0.0f32);
    for block in iq.chunks(4_000) {
        receiver.process(block, &mut frames);
        let mut tap = sdrmm_decoder::SymbolTap::default();
        tap.set_wanted(true);
        receiver.tap(&mut tap);
        assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
        assert_eq!(tap.reference.len(), 4);
        assert_eq!(tap.symbol_rate, f64::from(BAUD));
        seen += tap.symbols.len();
        worst = worst.max(tap.evm);
    }
    assert_eq!(frames.len(), 1);
    assert!(seen > 0);
    assert!(worst < 0.35, "measured {worst} EVM");
}

#[test]
fn noise_alone_decodes_nothing() {
    let mut receiver = HorusReceiver::new((-3_000.0, 3_000.0)).expect("receiver");
    let mut frames = Vec::new();
    let noise = sdrmm_decoder_test_support::complex_noise(3, 0.5, 80_000);
    receiver.process(&noise, &mut frames);
    assert!(frames.is_empty(), "{frames:?}");
}

#[test]
fn a_frame_shows_its_payload_where_it_is() {
    let mut frame = HorusFrame::new(3, "VK5ARG".to_owned(), 7);
    frame.place(-34.9, 138.6);
    frame.altitude_m = Some(12_000.0);
    frame.battery_v = Some(3.3);
    let view = frame.present(&EventKind::named("horus"));
    assert_eq!(view.entity.as_deref(), Some("VK5ARG"));
    let place = view.place.expect("a place");
    assert_eq!((place.at.lat, place.at.lon), (-34.9, 138.6));
    assert_eq!(
        view.field("Battery"),
        number(Some(3.3f32), Unit::Volts, 2).as_ref()
    );
    assert_eq!(
        view.field("Payload"),
        Some(&FieldValue::Text("VK5ARG".to_owned()))
    );
}

#[test]
fn a_frame_without_a_fix_has_no_place() {
    let mut frame = HorusFrame::new(1, "0".to_owned(), 9);
    frame.place(0.0, 0.0);
    assert!(frame.present(&EventKind::named("horus")).place.is_none());
}
