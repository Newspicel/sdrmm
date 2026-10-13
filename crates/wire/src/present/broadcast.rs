use super::{
    EventPayload, Field, FieldValue, Presentation, Unit, count, fields, flag, joined, number,
    only_if, text, text_of, trimmed,
};
use crate::{
    decode::{BroadcastData, BroadcastStatus, EventKind, RadioClockFrame, RdsUpdate},
    units::hertz,
};

const PLAIN_TEXT: &str = "text/plain";

impl EventPayload for RdsUpdate {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: trimmed(self.pi.as_deref()),
            label: trimmed(self.ps.as_deref()),
            fields: rds_fields(self),
            ..Presentation::of(rds_summary(self))
        }
    }
}

fn rds_fields(r: &RdsUpdate) -> Vec<Field> {
    let programme = r
        .pty_name
        .clone()
        .or_else(|| r.pty.map(|pty| format!("PTY {pty}")));
    let content = r.music.map(|music| if music { "music" } else { "speech" });
    let alternatives = r
        .alt_freqs_hz
        .iter()
        .map(|hz| hertz(*hz))
        .collect::<Vec<_>>()
        .join(", ");
    fields(&[
        ("PI", text(r.pi.as_deref())),
        ("Station", text(r.ps.as_deref())),
        ("Programme", text(programme.as_deref())),
        ("Traffic", flag(r.tp)),
        ("Announcement", flag(r.ta)),
        ("Content", content.and_then(text_of)),
        ("Alt. frequencies", text(Some(&alternatives))),
        ("Groups", count(Some(r.groups as f64))),
        ("Block errors", count(Some(r.block_errors as f64))),
        ("Radiotext", text(r.radiotext.as_deref())),
    ])
}

fn rds_summary(r: &RdsUpdate) -> String {
    let mut parts = Vec::new();
    parts.extend(r.pi.as_ref().map(|pi| format!("PI {pi}")));
    parts.extend(r.ps.clone());
    parts.extend(r.pty_name.clone());
    parts.extend(r.radiotext.clone());
    joined(parts)
}

impl EventPayload for BroadcastData {
    fn present(&self, _kind: EventKind) -> Presentation {
        let body = (self.media_type == PLAIN_TEXT).then(|| String::from_utf8_lossy(&self.bytes));
        Presentation {
            fields: fields(&[
                ("Name", text(Some(&self.name))),
                ("Type", text(Some(&self.media_type))),
                ("Bytes", count(Some(self.bytes.len() as f64))),
                ("Text", text(body.as_deref())),
            ]),
            ..Presentation::of(format!("{} · {} bytes", self.name, self.bytes.len()))
        }
    }
}

impl EventPayload for BroadcastStatus {
    fn present(&self, _kind: EventKind) -> Presentation {
        let system = self.system.label();
        let name = trimmed(self.label.as_deref()).unwrap_or_else(|| system.to_owned());
        let state = if self.locked { "locked" } else { "searching" };
        Presentation {
            fields: fields(&[
                ("System", text_of(system)),
                ("Locked", flag(Some(self.locked))),
                ("SNR", number(Some(self.snr_db), Unit::Db, 1)),
                ("Offset", number(Some(self.frequency_error_hz), Unit::Hz, 0)),
                ("Service", text(self.label.as_deref())),
                ("Ensemble", text(self.ensemble_label.as_deref())),
                (
                    "Bitrate",
                    self.bitrate_kbps
                        .map(|kbps| FieldValue::Text(format!("{kbps} kbit/s"))),
                ),
                ("Code rate", text(self.code_rate.as_deref())),
                ("Now", text(self.dynamic_label.as_deref())),
                ("Frames", count(Some(f64::from(self.frames_ok)))),
                ("Bad frames", count(Some(f64::from(self.frames_bad)))),
            ]),
            ..Presentation::of(format!("{name} {state} {:.1} dB", self.snr_db))
        }
    }
}

impl EventPayload for RadioClockFrame {
    fn present(&self, _kind: EventKind) -> Presentation {
        let standard = self.standard.label();
        let mut parts = vec![standard.to_owned(), self.datetime.clone()];
        if self.leap_warning {
            parts.push("leap warning".to_owned());
        }
        Presentation {
            entity: Some(standard.to_owned()),
            fields: radio_clock_fields(self, standard),
            ..Presentation::of(joined(parts))
        }
    }
}

fn radio_clock_fields(r: &RadioClockFrame, standard: &str) -> Vec<Field> {
    fields(&[
        ("Service", text_of(standard)),
        ("Civil time", text(Some(&r.datetime))),
        ("UTC offset", r.utc_offset_minutes.and_then(utc_offset)),
        ("DST", flag(Some(r.dst))),
        ("Leap warning", only_if(r.leap_warning)),
        ("DUT1", number(r.dut1_seconds, Unit::Seconds, 1)),
        ("Symbols", text(Some(&r.symbols))),
    ])
}

fn utc_offset(minutes: i16) -> Option<FieldValue> {
    let sign = if minutes < 0 { '-' } else { '+' };
    let absolute = minutes.unsigned_abs();
    text_of(format!(
        "UTC{sign}{:02}:{:02}",
        absolute / 60,
        absolute % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RadioClockStandard;

    #[test]
    fn rds_reads_as_fields_with_the_radiotext_last() {
        let view = serde_json::from_value::<RdsUpdate>(serde_json::json!({
            "groups": 100, "blocks": 402, "block_errors": 2, "pi": "D389", "ps": "RADIO 1 ",
            "pty_name": "Pop Music", "tp": true, "music": false,
            "alt_freqs_hz": [98_000_000.0, 100_500_000.0], "radiotext": "Now playing something",
        }))
        .unwrap()
        .present(EventKind::Rds);
        assert_eq!(view.field("PI"), text_of("D389").as_ref());
        assert_eq!(view.field("Station"), text_of("RADIO 1").as_ref());
        assert_eq!(view.field("Programme"), text_of("Pop Music").as_ref());
        assert_eq!(view.field("Traffic"), flag(Some(true)).as_ref());
        assert_eq!(view.field("Content"), text_of("speech").as_ref());
        assert_eq!(
            view.field("Alt. frequencies"),
            text_of("98 MHz, 100.5 MHz").as_ref()
        );
        assert_eq!(view.field("Groups"), count(Some(100)).as_ref());
        assert_eq!(view.field("Block errors"), count(Some(2)).as_ref());
        assert_eq!(view.labels().last(), Some(&"Radiotext"));
    }

    #[test]
    fn broadcast_text_is_shown_and_other_data_only_sized() {
        let text = BroadcastData {
            protocol: None,
            label: Vec::new(),
            service_id: None,
            name: "news.txt".to_owned(),
            media_type: PLAIN_TEXT.to_owned(),
            bytes: b"Headline".to_vec(),
        };
        let view = text.present(EventKind::BroadcastData);
        assert_eq!(view.field("Text"), text_of("Headline").as_ref());
        assert_eq!(view.field("Bytes"), count(Some(8)).as_ref());
        let picture = BroadcastData {
            media_type: "image/jpeg".to_owned(),
            ..text
        };
        assert_eq!(
            picture.present(EventKind::BroadcastData).field("Text"),
            None
        );
    }

    #[test]
    fn a_radio_clock_frame_shows_offset_and_dut1() {
        let view = RadioClockFrame {
            standard: RadioClockStandard::Dcf77,
            datetime: "2026-03-29 03:00".to_owned(),
            utc_offset_minutes: Some(120),
            dst: true,
            leap_warning: false,
            dut1_seconds: Some(-0.2),
            symbols: "0101".to_owned(),
        }
        .present(EventKind::RadioClock);
        assert_eq!(view.field("Service"), text_of("DCF77").as_ref());
        assert_eq!(view.field("UTC offset"), text_of("UTC+02:00").as_ref());
        assert_eq!(view.field("DST"), flag(Some(true)).as_ref());
        assert_eq!(view.field("Leap warning"), None);
        assert_eq!(utc_offset(-210), text_of("UTC-03:30"));
    }
}
