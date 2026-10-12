use serde_json::json;

use super::*;

fn group(
    station: &str,
    freq_hz: f64,
    count: u64,
    last: u32,
    airtime_ms: Option<u64>,
) -> DecoderLogGroup {
    serde_json::from_value(json!({
        "count": count,
        "first_at": "2026-05-01T11:00:00Z",
        "last_at": format!("2026-05-01T12:00:{last:02}Z"),
        "airtime_ms": airtime_ms,
        "latest": {
            "id": count, "at": format!("2026-05-01T12:00:{last:02}Z"), "device_set": 0,
            "channel": 1, "kind": "pocsag", "freq_hz": freq_hz, "station": station,
            "summary": "page",
            "event": { "kind": "pocsag", "data": {
                "address": 1, "function": 0, "baud": 1200, "payload": "numeric",
                "text": "1", "errors_corrected": 0,
            }},
        },
    }))
    .unwrap()
}

#[test]
fn views_map_to_and_from_a_group_key() {
    for (view, _) in VIEW_CHOICES {
        assert_eq!(LogView::of(view.group()), view);
    }
    assert_eq!(LogView::of(None), LogView::List);
}

#[test]
fn groups_label_by_frequency_or_station() {
    let groups = [group("123", 466_075_000.0, 4, 1, None)];
    let by_freq = group_rows(&groups, LogGroupKey::Frequency);
    assert_eq!(by_freq[0].label, "466.0750 MHz");
    assert_eq!(by_freq[0].key, "frequency:4660750");
    let by_station = group_rows(&groups, LogGroupKey::Station);
    assert_eq!(by_station[0].label, "123");
    assert_eq!(by_station[0].key, "station:pocsag:123");
}

#[test]
fn groups_sort_by_key_hits_or_recency() {
    let rows = group_rows(
        &[
            group("b", 300.0, 1, 3, None),
            group("A", 100.0, 9, 1, Some(1_500)),
            group("c", 200.0, 5, 2, None),
        ],
        LogGroupKey::Station,
    );
    let labels =
        |sorted: Vec<GroupRow>| sorted.into_iter().map(|row| row.label).collect::<Vec<_>>();
    assert_eq!(
        labels(sort_groups(&rows, GroupSort::Key, LogGroupKey::Station)),
        ["A", "b", "c"]
    );
    assert_eq!(
        labels(sort_groups(&rows, GroupSort::Count, LogGroupKey::Station)),
        ["A", "c", "b"]
    );
    assert_eq!(
        labels(sort_groups(&rows, GroupSort::Last, LogGroupKey::Station)),
        ["b", "c", "A"]
    );
    assert_eq!(
        labels(sort_groups(&rows, GroupSort::Key, LogGroupKey::Frequency)),
        ["A", "c", "b"]
    );
    assert!(has_airtime(&rows));
}

#[test]
fn airtime_reads_like_a_duration() {
    assert_eq!(format_airtime(None), "-");
    assert_eq!(format_airtime(Some(1_540)), "1.5 s");
    assert_eq!(format_airtime(Some(75_000)), "1:15");
    assert_eq!(format_airtime(Some(3_725_000)), "1:02:05");
}

#[test]
fn the_groups_path_names_the_key() {
    let query = DecoderLogQuery {
        sink: Some("log".to_owned()),
        ..DecoderLogQuery::default()
    };
    assert_eq!(
        groups_path(LogGroupKey::Station, &query),
        "/api/decoderlog/groups/station?sink=log"
    );
}
