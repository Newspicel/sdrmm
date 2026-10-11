use std::collections::HashMap;

use sdrmm_wire::{channel::ChannelParams, decode::DecoderEvent};

use super::detail::{course_of, heading_of, trimmed};
use crate::{
    decoders::views::lora_name,
    ui::{
        kit_maps::{
            feed::{MAP_KINDS, Station, event_position},
            tracks::Trail,
        },
        map::{
            Geo,
            geo::unwrap_trail,
            overlay::{Dot, Glyph, Ink, Label, Line, Mark, Overlay},
        },
    },
};

pub const TARGET_MAX_AGE_MS: i64 = 5 * 60_000;
const TRACK_WIDTH_PX: f64 = 1.5;
const TRACK_ALPHA: f32 = 0.7;
const LABEL_PX: f64 = 12.0;

#[must_use]
pub const fn style(kind: &str) -> (&'static str, u32) {
    match kind.as_bytes() {
        b"adsb" => ("Aircraft", 0x21_b0_b0),
        b"ais" => ("Ships", 0xe0_a4_58),
        b"radiosonde" => ("Sondes", 0xe0_6c_6c),
        b"lora" => ("LoRa", 0x6c_c0_70),
        b"remote_id" => ("Drones", 0xe0_d0_5c),
        _ => ("APRS", 0xb0_7d_e0),
    }
}

const fn glyph(kind: &str) -> Glyph {
    match kind.as_bytes() {
        b"adsb" => Glyph::Plane,
        b"ais" => Glyph::Ship,
        _ => Glyph::Arrow,
    }
}

const fn label_offset_em(kind: &str) -> f64 {
    match kind.as_bytes() {
        b"adsb" => 1.3,
        b"ais" => 1.1,
        _ => 0.7,
    }
}

#[must_use]
pub fn map_kinds_of(kinds: &[String]) -> Vec<&'static str> {
    MAP_KINDS
        .into_iter()
        .filter(|kind| kinds.iter().any(|wired| wired == kind))
        .collect()
}

#[must_use]
pub fn is_stale(last_seen: i64, now: i64, max_age_ms: i64) -> bool {
    last_seen < now - max_age_ms
}

#[must_use]
pub fn position(station: &Station) -> Option<Geo> {
    event_position(&station.event)
}

#[must_use]
pub fn label(station: &Station) -> String {
    match &station.event {
        DecoderEvent::Adsb(message) => {
            trimmed(message.callsign.as_deref()).unwrap_or_else(|| message.icao.to_uppercase())
        }
        DecoderEvent::Ais(message) => trimmed(message.name.as_deref())
            .or_else(|| trimmed(message.call_sign.as_deref()))
            .unwrap_or_else(|| message.mmsi.to_string()),
        DecoderEvent::Aprs(packet) => packet.source.clone(),
        DecoderEvent::Radiosonde(frame) => frame.serial.clone(),
        DecoderEvent::Lora(frame) => lora_name(frame).unwrap_or_else(|| station.id.clone()),
        DecoderEvent::RemoteId(frame) => frame.station(),
        _ => station.id.clone(),
    }
}

fn bearing(deg: Option<f64>) -> Option<f64> {
    deg.filter(|deg| deg.is_finite())
        .map(|deg| deg.rem_euclid(360.0))
}

#[must_use]
pub fn heading(station: &Station) -> Option<f64> {
    match &station.event {
        DecoderEvent::Adsb(message) => bearing(message.track_deg),
        DecoderEvent::Ais(message) => {
            bearing(heading_of(message.heading_deg)).or_else(|| bearing(course_of(message.cog_deg)))
        }
        DecoderEvent::Aprs(packet) => bearing(packet.course_deg),
        DecoderEvent::Radiosonde(frame) => bearing(frame.heading_deg),
        DecoderEvent::RemoteId(frame) => bearing(
            frame
                .location()
                .and_then(|location| location.track_deg)
                .map(f64::from),
        ),
        _ => None,
    }
}

#[must_use]
pub fn pick_key(kind: &str, id: &str) -> String {
    format!("{kind}/{id}")
}

#[must_use]
pub fn track_lines(
    stations: &[&Station],
    trails: Option<&HashMap<String, Trail>>,
    now: i64,
) -> Vec<Line> {
    let Some(trails) = trails else {
        return Vec::new();
    };
    stations
        .iter()
        .filter(|station| !is_stale(station.last_seen, now, TARGET_MAX_AGE_MS))
        .filter_map(|station| {
            let trail = trails.get(&station.id)?;
            (trail.points.len() >= 2).then(|| Line {
                points: unwrap_trail(&trail.points),
                colour: style(station.kind).1,
                alpha: TRACK_ALPHA,
                width: TRACK_WIDTH_PX,
                dash: None,
            })
        })
        .collect()
}

#[must_use]
pub fn overlay(stations: &[&Station], selected: Option<&str>, now: i64) -> (Overlay, usize) {
    let mut out = Overlay::default();
    let mut shown = 0;
    for station in stations {
        if is_stale(station.last_seen, now, TARGET_MAX_AGE_MS) {
            continue;
        }
        let Some(at) = position(station) else {
            continue;
        };
        shown += 1;
        let key = pick_key(station.kind, &station.id);
        let chosen = selected == Some(key.as_str());
        let (_, colour) = style(station.kind);
        out.dots.push(Dot {
            radius: vec![(0.0, if chosen { 6.0 } else { 4.0 })],
            stroke_width: if chosen { 3.0 } else { 1.0 },
            pick: Some(key.clone()),
            ..Dot::plain(at, 4.0, colour)
        });
        if let Some(heading) = heading(station) {
            out.marks.push(Mark {
                at,
                glyph: glyph(station.kind),
                heading,
                colour,
            });
        }
        out.labels.push(Label {
            key,
            at,
            text: label(station),
            ink: Ink::Ground,
            bold: true,
            below_px: label_offset_em(station.kind) * LABEL_PX,
            min_zoom: 0.0,
        });
    }
    (out, shown)
}

#[must_use]
pub fn references(params: &[ChannelParams]) -> Vec<Geo> {
    let mut out: Vec<Geo> = Vec::new();
    for param in params {
        let ChannelParams::Adsb(adsb) = param else {
            continue;
        };
        let Some(at) = Geo::checked(adsb.ref_lat, adsb.ref_lon) else {
            continue;
        };
        if !out.contains(&at) {
            out.push(at);
        }
    }
    out
}

#[must_use]
pub fn reference_marks(positions: &[Geo]) -> Vec<Mark> {
    positions
        .iter()
        .map(|at| Mark {
            at: *at,
            glyph: Glyph::Station,
            heading: 0.0,
            colour: crate::ui::map::ACCENT,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        channel::AdsbParams,
        decode::{AdsbMessage, AisMessage, AprsPacket},
    };

    use super::*;

    const NOW: i64 = 1_786_276_800_000;

    fn station(event: DecoderEvent) -> Station {
        Station {
            kind: event.kind(),
            id: "target".to_owned(),
            event,
            last_seen: NOW,
            freq_hz: 1_090_000_000.0,
            frames: 1,
        }
    }

    fn adsb(data: AdsbMessage) -> Station {
        station(DecoderEvent::Adsb(AdsbMessage {
            icao: "3c6444".to_owned(),
            df: 17,
            raw: "8d".to_owned(),
            ..data
        }))
    }

    fn ais(data: AisMessage) -> Station {
        station(DecoderEvent::Ais(AisMessage {
            mmsi: 211_234_560,
            msg_type: 1,
            ais_channel: 'A',
            nmea: "!AIVDM".to_owned(),
            ..data
        }))
    }

    fn aprs(data: AprsPacket) -> Station {
        station(DecoderEvent::Aprs(AprsPacket {
            source: "DL1ABC-9".to_owned(),
            destination: "APRS".to_owned(),
            info: "!".to_owned(),
            tnc2: "DL1ABC-9>APRS:!".to_owned(),
            ..data
        }))
    }

    #[test]
    fn only_decoders_that_report_a_position_are_mapped() {
        let wired = |kinds: &[&str]| {
            kinds
                .iter()
                .map(|kind| (*kind).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(map_kinds_of(&wired(&["adsb", "pocsag", "acars"])), ["adsb"]);
        assert!(map_kinds_of(&wired(&["pocsag", "rtty"])).is_empty());
        assert_eq!(
            map_kinds_of(&wired(&["aprs", "adsb", "aprs"])),
            ["adsb", "aprs"]
        );
        assert_eq!(
            map_kinds_of(&wired(&["ais", "adsb"])),
            map_kinds_of(&wired(&["adsb", "ais"]))
        );
    }

    #[test]
    fn a_target_has_a_position_label_and_heading() {
        let target = adsb(AdsbMessage {
            lat: Some(52.5163),
            lon: Some(13.3777),
            callsign: Some("DLH123 ".to_owned()),
            track_deg: Some(271.5),
            ..AdsbMessage::default()
        });
        assert_eq!(position(&target), Some(Geo::new(52.5163, 13.3777)));
        assert_eq!(label(&target), "DLH123");
        assert_eq!(heading(&target), Some(271.5));
        let still = adsb(AdsbMessage {
            lat: Some(1.0),
            lon: Some(2.0),
            ..AdsbMessage::default()
        });
        assert_eq!(heading(&still), None);
    }

    #[test]
    fn a_target_without_a_fix_is_not_drawn() {
        assert!(position(&adsb(AdsbMessage::default())).is_none());
        assert!(
            position(&adsb(AdsbMessage {
                lat: Some(52.5),
                ..AdsbMessage::default()
            }))
            .is_none()
        );
        assert!(
            position(&ais(AisMessage {
                lat: Some(91.0),
                lon: Some(181.0),
                ..AisMessage::default()
            }))
            .is_none()
        );
        assert!(
            position(&ais(AisMessage {
                lat: Some(0.0),
                lon: Some(0.0),
                ..AisMessage::default()
            }))
            .is_some()
        );
    }

    #[test]
    fn labels_fall_back_through_each_kinds_identities() {
        assert_eq!(
            label(&adsb(AdsbMessage {
                callsign: Some("  ".to_owned()),
                ..AdsbMessage::default()
            })),
            "3C6444"
        );
        assert_eq!(label(&ais(AisMessage::default())), "211234560");
        assert_eq!(
            label(&ais(AisMessage {
                call_sign: Some("DEAB".to_owned()),
                ..AisMessage::default()
            })),
            "DEAB"
        );
        assert_eq!(
            label(&ais(AisMessage {
                name: Some("NORDIC".to_owned()),
                call_sign: Some("DEAB".to_owned()),
                ..AisMessage::default()
            })),
            "NORDIC"
        );
        assert_eq!(label(&aprs(AprsPacket::default())), "DL1ABC-9");
    }

    #[test]
    fn a_vessel_prefers_true_heading_and_ignores_sentinels() {
        assert_eq!(
            heading(&ais(AisMessage {
                heading_deg: Some(90),
                cog_deg: Some(275.0),
                ..AisMessage::default()
            })),
            Some(90.0)
        );
        assert_eq!(
            heading(&ais(AisMessage {
                cog_deg: Some(275.0),
                ..AisMessage::default()
            })),
            Some(275.0)
        );
        assert_eq!(
            heading(&ais(AisMessage {
                heading_deg: Some(511),
                cog_deg: Some(360.0),
                ..AisMessage::default()
            })),
            None
        );
        assert_eq!(
            heading(&ais(AisMessage {
                heading_deg: Some(511),
                cog_deg: Some(12.0),
                ..AisMessage::default()
            })),
            Some(12.0)
        );
    }

    #[test]
    fn headings_wrap_into_a_circle() {
        let course = |deg| {
            heading(&aprs(AprsPacket {
                course_deg: Some(deg),
                ..AprsPacket::default()
            }))
        };
        assert_eq!(course(360.0), Some(0.0));
        assert_eq!(course(-90.0), Some(270.0));
        assert_eq!(course(450.0), Some(90.0));
        assert_eq!(course(f64::NAN), None);
    }

    #[test]
    fn a_target_expires_strictly_after_the_horizon() {
        assert!(!is_stale(
            NOW - TARGET_MAX_AGE_MS + 1,
            NOW,
            TARGET_MAX_AGE_MS
        ));
        assert!(is_stale(
            NOW - TARGET_MAX_AGE_MS - 1,
            NOW,
            TARGET_MAX_AGE_MS
        ));
        assert!(is_stale(NOW - 2_000, NOW, 1_000));
    }

    #[test]
    fn only_fresh_positioned_targets_are_drawn() {
        let fresh = Station {
            id: "fresh".to_owned(),
            ..adsb(AdsbMessage {
                lat: Some(1.0),
                lon: Some(1.0),
                ..AdsbMessage::default()
            })
        };
        let stale = Station {
            id: "stale".to_owned(),
            last_seen: NOW - TARGET_MAX_AGE_MS - 1,
            ..adsb(AdsbMessage {
                lat: Some(2.0),
                lon: Some(2.0),
                ..AdsbMessage::default()
            })
        };
        let unfixed = Station {
            id: "no-fix".to_owned(),
            ..adsb(AdsbMessage::default())
        };
        let (drawn, shown) = overlay(&[&fresh, &stale, &unfixed], None, NOW);
        assert_eq!(shown, 1);
        assert_eq!(drawn.dots[0].pick.as_deref(), Some("adsb/fresh"));
        let (empty, none) = overlay(&[], None, NOW);
        assert_eq!(none, 0);
        assert!(empty.dots.is_empty());
    }

    #[test]
    fn only_adsb_references_are_read_and_shared_antennas_merge() {
        let reference = |lat: Option<f64>, lon: Option<f64>| {
            ChannelParams::Adsb(AdsbParams {
                ref_lat: lat,
                ref_lon: lon,
                ..AdsbParams::default()
            })
        };
        assert_eq!(
            references(&[
                reference(Some(50.7), Some(6.1)),
                reference(Some(50.7), Some(6.1)),
                reference(Some(-33.9), Some(18.4))
            ]),
            [Geo::new(50.7, 6.1), Geo::new(-33.9, 18.4)]
        );
        assert!(
            references(&[
                reference(Some(50.7), None),
                reference(Some(91.0), Some(181.0))
            ])
            .is_empty()
        );
    }

    #[test]
    fn sondes_lora_nodes_and_drones_are_mapped_with_their_own_names() {
        use sdrmm_wire::{
            decode::DecoderEvent, remote_id::RemoteIdMessage, weather::RadiosondeFrame,
        };

        use crate::ui::kit_maps::fixtures::{drone, location, meshtastic, placed};

        assert_eq!(
            map_kinds_of(&[
                "remote_id".to_owned(),
                "lora".to_owned(),
                "radiosonde".to_owned()
            ]),
            ["radiosonde", "lora", "remote_id"]
        );
        let sonde: RadiosondeFrame = serde_json::from_value(serde_json::json!({
            "sonde": "rs41", "serial": "T1234567", "lat": 52.1, "lon": 13.2,
            "heading_deg": 450.0, "errors_corrected": 0
        }))
        .expect("a sonde");
        let sonde = station(DecoderEvent::Radiosonde(sonde));
        assert_eq!(label(&sonde), "T1234567");
        assert_eq!(heading(&sonde), Some(90.0));
        assert_eq!(position(&sonde), Some(Geo::new(52.1, 13.2)));
        let node = station(DecoderEvent::Lora(meshtastic(Some(placed(47.5, 8.25)))));
        assert_eq!(label(&node), "target");
        assert_eq!(heading(&node), None);
        assert_eq!(position(&node), Some(Geo::new(47.5, 8.25)));
        let quad = station(DecoderEvent::RemoteId(drone(
            None,
            vec![RemoteIdMessage::Location(location(47.0, 8.0))],
        )));
        assert_eq!(label(&quad), "D2:11:22:33:44:55");
        assert_eq!(heading(&quad), Some(90.0));
        assert_eq!(style("remote_id"), ("Drones", 0xe0_d0_5c));
    }

    #[test]
    fn tracks_are_drawn_only_for_fresh_targets_with_a_path() {
        let mut trails = HashMap::new();
        let mut trail = Trail::default();
        trail.extend(Geo::new(50.0, 8.0), 0);
        trail.extend(Geo::new(50.1, 8.0), 1);
        trails.insert("target".to_owned(), trail);
        let fresh = adsb(AdsbMessage::default());
        let lines = track_lines(&[&fresh], Some(&trails), NOW);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].colour, 0x21_b0_b0);
        assert_eq!((lines[0].width, lines[0].alpha), (1.5, 0.7));
        let stale = Station {
            last_seen: NOW - TARGET_MAX_AGE_MS - 1,
            ..fresh.clone()
        };
        assert!(track_lines(&[&stale], Some(&trails), NOW).is_empty());
        assert!(track_lines(&[&fresh], None, NOW).is_empty());
    }
}
