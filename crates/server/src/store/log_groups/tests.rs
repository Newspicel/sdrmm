use sdrmm_wire::{AdsbMessage, DecodedRecord, DecoderEvent, VoiceCall};

use super::*;
use crate::events::Routed;

fn call(freq_hz: f64, duration_ms: u64) -> DecoderEvent {
    DecoderEvent::Call(VoiceCall {
        id: 1,
        node: "nfm".to_owned(),
        started_at: "2026-10-05T10:00:00Z".to_owned(),
        ended_at: "2026-10-05T10:00:05Z".to_owned(),
        duration_ms,
        device_set: 0,
        channel: 0,
        freq_hz,
        mode: "nfm".to_owned(),
        slot: None,
        color_code: None,
        source: None,
        destination: None,
        group_call: None,
        encrypted: false,
        emergency: false,
        audio: None,
        audio_error: None,
    })
}

fn adsb(icao: &str) -> DecoderEvent {
    DecoderEvent::Adsb(AdsbMessage {
        icao: icao.to_owned(),
        df: 17,
        raw: "8D3C6444".to_owned(),
        ..AdsbMessage::default()
    })
}

fn heard(at: &str, freq_hz: f64, event: DecoderEvent) -> Routed {
    Routed::unattributed(DecodedRecord {
        origin: None,
        sinks: Vec::new(),
        device_set: 0,
        channel: 0,
        at: at.to_owned(),
        freq_hz,
        event,
    })
}

fn scanned() -> Store {
    let store = Store::open(None).expect("open");
    store
        .insert_decoder_events(&[
            heard(
                "2026-10-05T10:00:00Z",
                145_500_000.0,
                call(145_500_000.0, 4_000),
            ),
            heard(
                "2026-10-05T10:00:10Z",
                145_600_000.0,
                call(145_600_000.0, 2_000),
            ),
            heard(
                "2026-10-05T10:00:20Z",
                145_500_020.0,
                call(145_500_020.0, 3_000),
            ),
            heard("2026-10-05T10:00:30Z", 1_090_000_000.0, adsb("3C6444")),
            heard("2026-10-05T10:00:31Z", 1_090_000_000.0, adsb("4CA2D4")),
            heard("2026-10-05T10:00:32Z", 1_090_000_000.0, adsb("3C6444")),
        ])
        .expect("insert");
    store
}

#[test]
fn a_scan_reads_back_one_row_per_frequency_newest_first() {
    let (groups, total) = scanned()
        .group_decoder_log(&DecoderLogQuery::default(), LogGroupKey::Frequency)
        .expect("group");
    assert_eq!(total, 3);
    let rows: Vec<(f64, u64, Option<u64>)> = groups
        .iter()
        .map(|group| (group.latest.freq_hz, group.count, group.airtime_ms))
        .collect();
    assert_eq!(
        rows,
        vec![
            (1_090_000_000.0, 3, None),
            (145_500_020.0, 2, Some(7_000)),
            (145_600_000.0, 1, Some(2_000)),
        ]
    );
    assert_eq!(groups[1].first_at, "2026-10-05T10:00:00.000000000Z");
    assert_eq!(groups[1].last_at, "2026-10-05T10:00:20.000000000Z");
}

#[test]
fn planes_group_by_their_address() {
    let filter = DecoderLogQuery {
        kind: Some("adsb".to_owned()),
        ..DecoderLogQuery::default()
    };
    let (groups, total) = scanned()
        .group_decoder_log(&filter, LogGroupKey::Station)
        .expect("group");
    assert_eq!(total, 2);
    let rows: Vec<(Option<&str>, u64)> = groups
        .iter()
        .map(|group| (group.latest.station.as_deref(), group.count))
        .collect();
    assert_eq!(rows, vec![(Some("3C6444"), 2), (Some("4CA2D4"), 1)]);
}

#[test]
fn the_limit_bounds_the_rows_but_not_the_total() {
    let filter = DecoderLogQuery {
        limit: Some(1),
        ..DecoderLogQuery::default()
    };
    let (groups, total) = scanned()
        .group_decoder_log(&filter, LogGroupKey::Station)
        .expect("group");
    assert_eq!(groups.len(), 1);
    assert_eq!(total, 3);
}
