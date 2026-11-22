use sdrmm_decoder::aircraft::AdsbMessage;
use sdrmm_decoder_test_support::{cf32_le, settings};
use sdrmm_modem::synth::add_noise;
use sdrmm_wire::ChannelParams;

use super::synth::{Burst, METAR, N70FC, long_frame, text_uplink, transmission};
use super::*;

fn channel(p: &UatParams) -> UatChannel {
    UatChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::of(p).expect("settings")),
    )
    .expect("uat channel")
}

#[derive(Default, Debug)]
struct Heard {
    aircraft: Vec<AdsbMessage>,
    reports: Vec<FisbReport>,
}

fn feed(chan: &mut UatChannel, iq: &[Complex<f32>], blocks: &[usize]) -> Heard {
    let mut out = ChannelOutputs::default();
    let mut heard = Heard::default();
    let mut at = 0;
    for len in blocks.iter().cycle() {
        if at >= iq.len() {
            break;
        }
        let end = (at + len).min(iq.len());
        out.reset();
        chan.process(&iq[at..end], &mut out);
        for event in &out.events {
            if let Some(aircraft) = event.read::<AdsbMessage>() {
                heard.aircraft.push(aircraft);
            } else if let Some(report) = event.read::<FisbReport>() {
                heard.reports.push(report);
            } else {
                panic!("uat emitted {}", event.kind());
            }
        }
        at = end;
    }
    heard
}

fn hear(iq: &[Complex<f32>]) -> (Heard, u32) {
    let mut chan = channel(&UatParams::default());
    let heard = feed(&mut chan, iq, &[997, 16_384, 4_096, 1]);
    (heard, chan.rejected())
}

#[test]
fn an_aircraft_and_a_metar_round_trip_through_the_air() {
    let iq = synth::scene(INPUT_RATE_HZ);
    let (heard, rejected) = hear(&iq);
    assert_eq!(rejected, 0);
    let [aircraft] = heard.aircraft.as_slice() else {
        panic!("{heard:?}");
    };
    assert_eq!(aircraft.icao, "A952B5");
    assert_eq!(aircraft.callsign.as_deref(), Some("N70FC"));
    assert_eq!(aircraft.altitude_ft, Some(2_500));
    assert_eq!(aircraft.vertical_rate_fpm, Some(512));
    assert_eq!(aircraft.on_ground, Some(false));
    assert!((aircraft.lat.unwrap_or_default() - N70FC.lat).abs() < 1e-4);
    assert!((aircraft.lon.unwrap_or_default() - N70FC.lon).abs() < 1e-4);
    assert!((aircraft.ground_speed_kt.unwrap_or_default() - 98.49).abs() < 0.01);
    assert!((aircraft.track_deg.unwrap_or_default() - 113.96).abs() < 0.01);
    assert_eq!(
        heard.reports,
        [FisbReport {
            product_id: 413,
            product: "METAR".to_owned(),
            location: Some("KPAO".to_owned()),
            issued: Some("241547Z".to_owned()),
            text: "31008KT 10SM CLR 14/02 A3012=".to_owned(),
        }]
    );
}

#[test]
fn noisy_bursts_off_frequency_still_decode() {
    let mut iq = synth::scene(INPUT_RATE_HZ);
    sdrmm_modem::synth::shift(&mut iq, 40_000.0, INPUT_RATE_HZ);
    add_noise(&mut iq, 7, 0.25);
    let (heard, _) = hear(&iq);
    assert_eq!(heard.aircraft.len(), 1, "{heard:?}");
    assert_eq!(heard.reports.len(), 1, "{heard:?}");
}

#[test]
fn the_scope_reads_a_downlink_as_two_tight_levels_at_the_bit_rate() {
    let iq = transmission(&[Burst::Downlink(long_frame(&N70FC))], INPUT_RATE_HZ);
    let mut chan = channel(&UatParams::default());
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    chan.process(&iq, &mut out);
    let tap = &out.symbols;
    assert_eq!(out.events.len(), 1);
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert_eq!(tap.symbols.len(), fec::LONG_BYTES * 8);
    assert_eq!(tap.symbol_rate, BIT_RATE_HZ);
    assert!(tap.evm < 0.15, "EVM {}", tap.evm);
}

#[test]
fn noise_alone_produces_nothing() {
    let mut iq = vec![Complex::new(0.0, 0.0); 2_000_000];
    add_noise(&mut iq, 3, 0.5);
    let (heard, _) = hear(&iq);
    assert!(heard.aircraft.is_empty() && heard.reports.is_empty());
}

#[test]
fn a_repeated_report_is_passed_on_once() {
    let uplink = Burst::Uplink(text_uplink(&[METAR]));
    let iq = transmission(&[uplink.clone(), uplink], INPUT_RATE_HZ);
    let (heard, _) = hear(&iq);
    assert_eq!(heard.reports.len(), 1);
}

#[test]
fn fisb_off_keeps_only_the_aircraft() {
    let mut chan = channel(&UatParams { fisb: false });
    let heard = feed(&mut chan, &synth::scene(INPUT_RATE_HZ), &[8_192]);
    assert_eq!(heard.aircraft.len(), 1);
    assert!(heard.reports.is_empty());
}

#[test]
fn a_burst_the_code_cannot_repair_is_counted() {
    let mut iq = transmission(&[Burst::Downlink(long_frame(&N70FC))], INPUT_RATE_HZ);
    let start = (0.002 * INPUT_RATE_HZ) as usize + 72;
    for sample in &mut iq[start..start + 2 * 8 * 20] {
        *sample = sample.conj();
    }
    let (heard, rejected) = hear(&iq);
    assert!(heard.aircraft.is_empty());
    assert!(rejected >= 1);
}

#[test]
fn refuses_every_rate_but_its_own() {
    let refused = UatChannel::new(
        ChannelCtx {
            input_rate: 2_400_000.0,
        },
        settings(ChannelParams::of(&UatParams::default()).expect("settings")),
    );
    assert!(refused.is_err());
}

/// `fixtures/uat_reference_2m083`, built by `uat_reference.py`: dump978 decodes it to the same
/// twelve frames, with the same repairs, that its sources hold.
fn reference() -> Vec<Complex<f32>> {
    cf32_le(include_bytes!(
        "../../../../fixtures/uat_reference_2m083.sigmf-data"
    ))
}

fn aircraft<'a>(heard: &'a Heard, icao: &str) -> &'a AdsbMessage {
    heard
        .aircraft
        .iter()
        .find(|aircraft| aircraft.icao == icao)
        .unwrap_or_else(|| panic!("no {icao}: {heard:?}"))
}

#[test]
fn the_reference_fixture_holds_every_aircraft() {
    let (heard, rejected) = hear(&reference());
    assert_eq!(rejected, 0);
    let addresses: Vec<&str> = heard.aircraft.iter().map(|a| a.icao.as_str()).collect();
    assert_eq!(
        addresses,
        ["A66EF1", "A952B5", "AD7233", "ABA4BE", "AC46A0"]
    );
    let basic = aircraft(&heard, "A66EF1");
    assert_eq!(basic.altitude_ft, Some(1_000));
    assert_eq!(basic.vertical_rate_fpm, Some(-192));
    assert_eq!(
        aircraft(&heard, "A952B5").callsign.as_deref(),
        Some("N70FC")
    );
    let squawking = aircraft(&heard, "AD7233");
    assert_eq!(squawking.squawk.as_deref(), Some("0325"));
    assert_eq!(squawking.altitude_ft, Some(3_175));
    let tisb = aircraft(&heard, "ABA4BE");
    assert_eq!(tisb.callsign.as_deref(), Some("N8498F"));
    assert_eq!(tisb.altitude_ft, Some(2_725));
    assert!(
        (tisb.lat.unwrap_or_default() - 40.0047).abs() < 1e-4,
        "{tisb:?}"
    );
    assert!(
        (tisb.lon.unwrap_or_default() + 86.2266).abs() < 1e-4,
        "{tisb:?}"
    );
    assert!(
        (tisb.track_deg.unwrap_or_default() - 169.4).abs() < 0.1,
        "{tisb:?}"
    );
    assert_eq!(tisb.vertical_rate_fpm, Some(1_024));
}

fn report<'a>(heard: &'a Heard, product: &str, location: &str) -> &'a FisbReport {
    heard
        .reports
        .iter()
        .find(|report| report.product == product && report.location.as_deref() == Some(location))
        .unwrap_or_else(|| panic!("no {product} {location}: {:#?}", heard.reports))
}

#[test]
fn the_reference_fixture_reads_the_metars_dump978_reads() {
    let (heard, _) = hear(&reference());
    let kpoc = report(&heard, "METAR", "KPOC");
    assert_eq!(kpoc.issued.as_deref(), Some("240347Z"));
    assert_eq!(kpoc.text, "00000KT 10SM CLR 11/M02 A3017=");
    assert_eq!(
        report(&heard, "METAR", "KMHV").text,
        "AUTO 32007KT 10SM CLR 07/M05 A3028 RMK AO1="
    );
    let noisy = report(&heard, "METAR", "KSPI");
    assert_eq!(noisy.issued.as_deref(), Some("070652Z"));
    assert!(
        noisy
            .text
            .starts_with("AUTO 36014KT 10SM -RA BKN007 OVC012 02/01 A2986 RMK"),
        "{noisy:?}"
    );
    assert!(
        noisy
            .text
            .ends_with("AO2 CIG 005V009 SLP113 P0006 T00220006 $=")
    );
    for station in ["KGSH", "KLHQ", "KPAH"] {
        report(&heard, "METAR", station);
    }
}

#[test]
fn the_reference_fixture_reads_notam_taf_and_airspace_text() {
    let (heard, _) = hear(&reference());
    let oak = report(&heard, "NOTAM-D", "KOAK");
    assert_eq!(oak.product_id, 8);
    assert_eq!(oak.issued.as_deref(), Some("01/23 23:53"));
    assert_eq!(
        oak.text,
        "NOTAM-D KOAK.01/115 232353Z !OAK 01/115 OAK RWY 15/33 CLSD 1501232353-1501250200"
    );
    assert_eq!(
        report(&heard, "NOTAM-D", "KDAY").text,
        "NOTAM-D KDAY.03/025 041234Z !DAY 03/025 DAY NAV ILS RWY 18 GP U/S 2203041234-2203172000EST"
    );
    let taf = report(&heard, "TAF", "KJEF");
    assert_eq!(taf.issued.as_deref(), Some("120520Z"));
    assert!(
        taf.text.starts_with("1206/1306 32010KT P6SM SKC"),
        "{taf:?}"
    );
    let sua: Vec<&FisbReport> = heard
        .reports
        .iter()
        .filter(|r| r.product == "SUA")
        .collect();
    assert_eq!(sua.len(), 4, "{sua:#?}");
    assert!(sua.iter().all(|r| r.location.is_none()), "{sua:#?}");
    assert!(sua[0].text.contains("ABEL SOUTH MOA"), "{sua:#?}");
    assert_eq!(heard.reports.len(), 19, "{:#?}", heard.reports);
}
