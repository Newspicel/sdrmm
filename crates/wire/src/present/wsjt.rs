use super::{EventPayload, Field, Presentation, Spot, Unit, count, fields, number, text, trimmed};
use crate::{
    LatLon,
    decode::{EventKind, WsjtMessage, WsprSpot},
};

const RR73: &str = "RR73";

impl EventPayload for WsjtMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        let sender = wsjt_sender(&self.text).map(str::to_owned);
        Presentation {
            entity: sender.clone(),
            callsign: sender,
            spot: message_spot(&self.text, self.snr_db),
            fields: wsjt_fields(self),
            ..Presentation::of(format!(
                "{} · {:+.0} dB · {:.0} Hz",
                self.text, self.snr_db, self.audio_hz
            ))
        }
    }
}

impl EventPayload for WsprSpot {
    fn present(&self, _kind: EventKind) -> Presentation {
        let callsign = trimmed(Some(&self.callsign));
        Presentation {
            entity: callsign.clone(),
            spot: self
                .grid
                .as_deref()
                .and_then(|grid| spot(grid, callsign.clone().unwrap_or_default(), self.snr_db)),
            callsign,
            fields: wspr_fields(self),
            ..Presentation::of(format!(
                "{} · {:+.0} dB · {:.0} Hz",
                self.text, self.snr_db, self.audio_hz
            ))
        }
    }
}

fn wsjt_fields(m: &WsjtMessage) -> Vec<Field> {
    fields(&[
        ("SNR", number(Some(m.snr_db), Unit::Db, 0)),
        ("Audio", number(Some(m.audio_hz), Unit::Hz, 1)),
        (
            "Time offset",
            number(Some(m.time_offset_s), Unit::Seconds, 2),
        ),
        ("Hard errors", count(Some(m.hard_errors))),
        ("Message", text(Some(&m.text))),
    ])
}

fn wspr_fields(s: &WsprSpot) -> Vec<Field> {
    fields(&[
        ("Callsign", text(Some(&s.callsign))),
        ("Grid", text(s.grid.as_deref())),
        ("Power", number(Some(s.power_dbm), Unit::Dbm, 0)),
        ("SNR", number(Some(s.snr_db), Unit::Db, 0)),
        ("Audio", number(Some(s.audio_hz), Unit::Hz, 1)),
        (
            "Time offset",
            number(Some(s.time_offset_s), Unit::Seconds, 2),
        ),
        ("Drift", number(Some(s.drift_hz), Unit::Hz, 1)),
        ("Message", text(Some(&s.text))),
    ])
}

fn wsjt_sender(text: &str) -> Option<&str> {
    let mut words = text.split_whitespace();
    if words.next()? != "CQ" {
        return words.next();
    }
    words.find(|word| {
        word.bytes().any(|b| b.is_ascii_digit()) && word.bytes().any(|b| b.is_ascii_alphabetic())
    })
}

fn message_spot(text: &str, snr_db: f32) -> Option<Spot> {
    let upper = text.to_ascii_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    let (grid, before) = match words.as_slice() {
        [.., before, grid] => (*grid, Some(*before)),
        [grid] => (*grid, None),
        [] => return None,
    };
    if grid == RR73 || !is_square(grid) {
        return None;
    }
    let callsign = before
        .map(|word| word.trim_start_matches('<').trim_end_matches('>'))
        .unwrap_or_default();
    spot(grid, callsign.to_owned(), snr_db)
}

fn is_square(word: &str) -> bool {
    matches!(
        word.as_bytes(),
        [b'A'..=b'R', b'A'..=b'R', b'0'..=b'9', b'0'..=b'9']
    )
}

fn spot(grid: &str, callsign: String, snr_db: f32) -> Option<Spot> {
    let grid = grid.trim().to_ascii_uppercase();
    Some(Spot {
        at: grid_lat_lon(&grid)?,
        grid,
        callsign,
        snr_db,
    })
}

#[must_use]
pub fn grid_lat_lon(grid: &str) -> Option<LatLon> {
    let text = grid.trim().to_ascii_uppercase();
    let bytes = text.as_bytes();
    if !matches!(bytes.len(), 4 | 6 | 8) {
        return None;
    }
    let mut lon = 0.0;
    let mut lat = 0.0;
    let mut lon_size = 360.0;
    let mut lat_size = 180.0;
    for (pair, &[east, north]) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let (base, count) = if pair % 2 == 0 {
            (b'A', if pair == 0 { 18 } else { 24 })
        } else {
            (b'0', 10)
        };
        let east = east.checked_sub(base).filter(|step| *step < count)?;
        let north = north.checked_sub(base).filter(|step| *step < count)?;
        lon_size /= f64::from(count);
        lat_size /= f64::from(count);
        lon += f64::from(east) * lon_size;
        lat += f64::from(north) * lat_size;
    }
    Some(LatLon {
        lat: lat + lat_size / 2.0 - 90.0,
        lon: lon + lon_size / 2.0 - 180.0,
    })
}

#[must_use]
pub fn square_of(at: LatLon) -> String {
    let lon = (at.lon + 180.0).clamp(0.0, 359.999_999);
    let lat = (at.lat + 90.0).clamp(0.0, 179.999_999);
    let letter = |step: f64| char::from(b'A' + step as u8);
    let digit = |step: f64| char::from(b'0' + step as u8);
    [
        letter((lon / 20.0).floor()),
        letter((lat / 10.0).floor()),
        digit(((lon % 20.0) / 2.0).floor()),
        digit((lat % 10.0).floor()),
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_round_trips_through_its_centre() {
        for grid in ["FN42", "JO62", "IO91", "PM95", "AA00", "RR99"] {
            let centre = grid_lat_lon(grid).expect(grid);
            assert_eq!(square_of(centre), grid);
        }
    }

    #[test]
    fn wspr_shows_timing_and_link_measurements() {
        let view = serde_json::from_value::<WsprSpot>(serde_json::json!({
            "text": "K1ABC FN42 37", "callsign": "K1ABC", "grid": "FN42", "power_dbm": 37,
            "snr_db": -21.0, "audio_hz": 1501.25, "time_offset_s": 0.75, "drift_hz": -0.2,
        }))
        .unwrap()
        .present(EventKind::Wspr);
        assert_eq!(
            view.labels(),
            [
                "Callsign",
                "Grid",
                "Power",
                "SNR",
                "Audio",
                "Time offset",
                "Drift",
                "Message"
            ]
        );
        assert_eq!(view.field("Power"), number(Some(37), Unit::Dbm, 0).as_ref());
        assert_eq!(
            view.field("Time offset"),
            number(Some(0.75_f32), Unit::Seconds, 2).as_ref()
        );
        assert_eq!(view.field("Message"), text(Some("K1ABC FN42 37")).as_ref());
    }

    #[test]
    fn ft8_counts_hard_errors() {
        let view = WsjtMessage {
            text: "CQ DL1ABC JO62".to_owned(),
            snr_db: -12.0,
            audio_hz: 1234.5,
            time_offset_s: 0.1,
            hard_errors: 3,
        }
        .present(EventKind::Ft8);
        assert_eq!(view.field("Hard errors"), count(Some(3)).as_ref());
        assert_eq!(
            view.field("SNR"),
            number(Some(-12.0_f32), Unit::Db, 0).as_ref()
        );
    }
}
