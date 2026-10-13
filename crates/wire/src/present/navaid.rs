use super::{
    EventPayload, Field, Presentation, Unit, count, fields, joined, number, percent, position,
    text, text_of, trimmed,
};
use crate::{
    channel::IlsComponent,
    decode::{EventKind, GnssFrame, IlsReading, VorReading},
};

impl EventPayload for VorReading {
    fn present(&self, _kind: EventKind) -> Presentation {
        let station = self.station.as_deref().unwrap_or("VOR");
        Presentation {
            entity: trimmed(self.station.as_deref()),
            fields: vor_fields(self),
            ..Presentation::of(format!(
                "{station} · {:03.1}° radial · {:.0}%",
                self.radial_deg,
                self.confidence * 100.0
            ))
        }
    }
}

fn vor_fields(v: &VorReading) -> Vec<Field> {
    fields(&[
        ("Station", text(v.station.as_deref())),
        ("Radial", number(Some(v.radial_deg), Unit::Degrees, 2)),
        (
            "Variable phase",
            number(Some(v.variable_phase_deg), Unit::Degrees, 2),
        ),
        (
            "Reference phase",
            number(Some(v.reference_phase_deg), Unit::Degrees, 2),
        ),
        (
            "Declination",
            number(Some(v.magnetic_declination_deg), Unit::Degrees, 1),
        ),
        ("Station position", position(v.station_lat, v.station_lon)),
        ("Signal", number(Some(v.signal_db), Unit::Db, 1)),
        ("Confidence", percent(v.confidence)),
    ])
}

impl EventPayload for IlsReading {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            fields: ils_fields(self),
            ..Presentation::of(format!(
                "{} · {:+.3} DDM · {:+.2} dots",
                ils_component(self.component),
                self.ddm,
                self.deviation_dots
            ))
        }
    }
}

fn ils_component(component: IlsComponent) -> &'static str {
    match component {
        IlsComponent::Localizer => "localizer",
        IlsComponent::Glideslope => "glideslope",
    }
}

fn ils_fields(i: &IlsReading) -> Vec<Field> {
    fields(&[
        ("Component", text_of(ils_component(i.component))),
        (
            "90 Hz",
            number(Some(i.modulation_90 * 100.0), Unit::Percent, 2),
        ),
        (
            "150 Hz",
            number(Some(i.modulation_150 * 100.0), Unit::Percent, 2),
        ),
        ("DDM", number(Some(i.ddm), Unit::None, 4)),
        ("Dots", number(Some(i.deviation_dots), Unit::None, 2)),
        ("Signal", number(Some(i.signal_db), Unit::Db, 1)),
    ])
}

impl EventPayload for GnssFrame {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: Some(format!("GPS-{}", self.prn)),
            fields: gnss_fields(self),
            ..Presentation::of(gnss_summary(self))
        }
    }
}

fn gnss_fields(g: &GnssFrame) -> Vec<Field> {
    let subframe = g
        .subframe
        .map_or_else(|| text_of("acquiring"), |id| count(Some(id)));
    fields(&[
        ("Signal", text_of(format!("GPS L1 C/A PRN {}", g.prn))),
        ("Doppler", number(Some(g.doppler_hz), Unit::Hz, 0)),
        (
            "Code phase",
            number(Some(g.code_phase_chips), Unit::Chips, 2),
        ),
        ("C/N0", number(Some(g.cn0_db_hz), Unit::DbHz, 1)),
        ("Subframe", subframe),
        ("Time of week", number(g.tow_seconds, Unit::Seconds, 0)),
        ("Week", count(g.week)),
        ("Words", text(Some(&g.words.join(" ")))),
    ])
}

fn gnss_summary(g: &GnssFrame) -> String {
    let mut parts = vec![
        format!("GPS PRN {}", g.prn),
        format!("{:+.0} Hz", g.doppler_hz),
        format!("{:.1} dB-Hz", g.cn0_db_hz),
    ];
    if let Some(id) = g.subframe {
        parts.push(format!("subframe {id}"));
    } else {
        parts.push("acquired".to_owned());
    }
    if let Some(tow) = g.tow_seconds {
        parts.push(format!("TOW {tow} s"));
    }
    joined(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LatLon, present::FieldValue};

    #[test]
    fn vor_shows_its_phases_and_station() {
        let reading = VorReading {
            station: Some("BOT".to_owned()),
            station_lat: Some(51.5),
            station_lon: Some(7.1),
            magnetic_declination_deg: -2.5,
            radial_deg: 123.456,
            variable_phase_deg: 10.0,
            reference_phase_deg: 20.0,
            signal_db: 30.25,
            confidence: 0.874,
        };
        let view = reading.present(EventKind::Vor);
        assert_eq!(
            view.labels(),
            [
                "Station",
                "Radial",
                "Variable phase",
                "Reference phase",
                "Declination",
                "Station position",
                "Signal",
                "Confidence"
            ]
        );
        assert_eq!(
            view.field("Station position"),
            Some(&FieldValue::Position(LatLon {
                lat: 51.5,
                lon: 7.1
            }))
        );
        assert_eq!(view.field("Confidence"), percent(0.874).as_ref());
    }

    #[test]
    fn ils_names_component_and_modulation_depths() {
        let reading = IlsReading {
            component: IlsComponent::Glideslope,
            modulation_90: 0.4,
            modulation_150: 0.38,
            ddm: 0.02,
            deviation_dots: -0.25,
            signal_db: 12.0,
        };
        let view = reading.present(EventKind::Ils);
        assert_eq!(view.field("Component"), text(Some("glideslope")).as_ref());
        assert_eq!(
            view.field("90 Hz"),
            number(Some(0.4_f32 * 100.0), Unit::Percent, 2).as_ref()
        );
        assert_eq!(
            view.field("Dots"),
            number(Some(-0.25), Unit::None, 2).as_ref()
        );
    }

    #[test]
    fn gnss_says_when_it_is_still_acquiring() {
        let frame = GnssFrame {
            prn: 12,
            doppler_hz: -1234.0,
            code_phase_chips: 511.5,
            cn0_db_hz: 44.25,
            subframe: None,
            tow_seconds: None,
            week: None,
            words: Vec::new(),
        };
        let view = frame.present(EventKind::Gnss);
        assert_eq!(
            view.labels(),
            ["Signal", "Doppler", "Code phase", "C/N0", "Subframe"]
        );
        assert_eq!(view.field("Subframe"), text(Some("acquiring")).as_ref());
        assert_eq!(
            view.field("C/N0"),
            number(Some(44.25), Unit::DbHz, 1).as_ref()
        );
        assert_eq!(
            view.field("Code phase"),
            number(Some(511.5), Unit::Chips, 2).as_ref()
        );
    }
}
