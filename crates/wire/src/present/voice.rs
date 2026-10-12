use super::{
    EventPayload, Field, FieldValue, Place, Presentation, Unit, VoiceFacts, count, fields, flag,
    joined, lat_lon, measure::modulation_label, number, only_if, percent, position, text, text_of,
    trimmed,
};
use crate::{
    decode::{
        DvFrame, DvFrameKind, DvMode, DvTrunkProtocol, EventKind, ScramblerStatus,
        ToneSquelchStatus,
    },
    monitor::Transmission,
    rest::VoiceCall,
    units::hertz,
};

impl EventPayload for ToneSquelchStatus {
    fn present(&self, _kind: EventKind) -> Presentation {
        let mut parts = Vec::new();
        if let Some(hz) = self.ctcss_hz {
            parts.push(format!("CTCSS {hz:.1} Hz"));
        }
        if let Some(code) = self.dcs_code {
            parts.push(format!("DCS {code:03}"));
        }
        if parts.is_empty() {
            parts.push("no tone".to_owned());
        }
        let squelch = if self.open { "open" } else { "muted" };
        parts.push(squelch.to_owned());
        Presentation {
            fields: fields(&[
                ("CTCSS", number(self.ctcss_hz, Unit::Hz, 1)),
                (
                    "DCS",
                    self.dcs_code.and_then(|code| text_of(format!("{code:03}"))),
                ),
                ("Audio", text_of(squelch)),
            ]),
            ..Presentation::of(joined(parts))
        }
    }
}

impl EventPayload for ScramblerStatus {
    fn present(&self, _kind: EventKind) -> Presentation {
        let found = self.inversion_hz.is_some();
        Presentation {
            fields: fields(&[
                ("Inversion", number(self.inversion_hz, Unit::Hz, 0)),
                (
                    "Confidence",
                    percent(self.confidence as f32).filter(|_| found),
                ),
            ]),
            ..Presentation::of(match self.inversion_hz {
                Some(hz) => format!(
                    "inversion {hz:.0} Hz · {:.0}% confidence",
                    self.confidence * 100.0
                ),
                None => "no inversion".to_owned(),
            })
        }
    }
}

impl EventPayload for DvFrame {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: trimmed(self.source_call.as_deref())
                .or_else(|| self.source.map(|id| id.to_string())),
            place: lat_lon(self.lat, self.lon).map(Place::at),
            voice: Some(VoiceFacts {
                source: self.source,
                destination: self.destination,
                encrypted: self.encrypted,
                emergency: self.emergency,
            }),
            callsign: trimmed(self.source_call.as_deref()),
            fields: dv_fields(self),
            ..Presentation::of(dv_summary(self))
        }
    }
}

fn dv_fields(f: &DvFrame) -> Vec<Field> {
    let mut shown = fields(&[
        ("Mode", text_of(f.mode.label())),
        ("Frame", text_of(dv_frame_kind(f.kind))),
        ("Network", text(Some(&dv_network(f)))),
        ("Vendor", dv_vendor(f)),
        ("Trunking", text(Some(&dv_trunking(f)))),
        ("Parties", text(f.parties().as_deref())),
        ("Talker alias", text(f.talker_alias.as_deref())),
        (
            "Call",
            f.group_call.and_then(|group| text_of(call_type(group))),
        ),
        ("Via", text(f.via.as_deref())),
        ("Signalling", text(f.opcode.as_deref())),
        ("Position", position(f.lat, f.lon)),
        (
            "Position error",
            number(f.position_error_m, Unit::Metres, 0),
        ),
        ("Channel", count(f.channel)),
        ("Channel frequency", dv_channel_definition(f)),
        ("Rest channel", count(f.rest_channel)),
    ]);
    shown.extend(dv_site_fields(f));
    shown.extend(fields(&[
        ("Emergency", flag(f.emergency)),
        ("Late entry", flag(f.late_entry)),
        ("Slot activity", text(Some(&slot_activity(f)))),
        ("Encrypted", flag(f.encrypted)),
        (
            "Algorithm",
            f.algorithm_id.and_then(|id| text_of(hex(id, 2))),
        ),
        ("Key ID", f.key_id.and_then(|id| text_of(hex(id, 4)))),
        ("Message indicator", text(f.message_indicator.as_deref())),
        (
            "Repaired",
            count(Some(f.errors_corrected).filter(|bits| *bits > 0)),
        ),
        (
            "Checksum",
            f.crc_verified.and_then(|ok| text_of(checksum(ok))),
        ),
        ("Text", text(f.text.as_deref())),
        ("Data", text(f.data.as_deref())),
    ]));
    shown
}

fn dv_site_fields(f: &DvFrame) -> Vec<Field> {
    let (network, system, site) = if f.mode == DvMode::Tetra {
        ("MNC", "MCC", "Location area")
    } else {
        ("Network ID", "System ID", "Site ID")
    };
    fields(&[
        (network, f.network_id.and_then(text_of)),
        (system, f.system_id.and_then(text_of)),
        (site, f.site_id.and_then(text_of)),
    ])
}

fn dv_frame_kind(kind: DvFrameKind) -> &'static str {
    match kind {
        DvFrameKind::Header => "call",
        DvFrameKind::Voice => "in progress",
        DvFrameKind::Terminator => "end",
        DvFrameKind::Control => "signalling",
        DvFrameKind::Data => "data",
    }
}

fn call_type(group: bool) -> &'static str {
    if group { "talkgroup" } else { "private" }
}

fn checksum(verified: bool) -> &'static str {
    if verified {
        "verified"
    } else {
        "not verified: read on error correction alone"
    }
}

fn hex(value: impl Into<u32>, width: usize) -> String {
    format!("0x{:0width$X}", value.into())
}

fn dv_network(f: &DvFrame) -> String {
    let mut parts = Vec::new();
    parts.extend(f.slot.map(|slot| format!("TS{slot}")));
    parts.extend(color_code(f));
    parts.join(" ")
}

fn color_code(f: &DvFrame) -> Option<String> {
    f.color_code.map(|cc| match f.mode {
        DvMode::P25 => format!("NAC {cc:03X}"),
        DvMode::Nxdn | DvMode::Dpmr => format!("RAN {cc}"),
        _ => format!("CC {cc}"),
    })
}

fn dv_vendor(f: &DvFrame) -> Option<FieldValue> {
    let vendor = f.vendor?.label();
    match f.manufacturer_id {
        Some(id) => text_of(format!("{vendor} ({})", hex(id, 2))),
        None => text_of(vendor),
    }
}

fn dv_trunking(f: &DvFrame) -> String {
    let system = f.trunk_protocol.map(|protocol| match protocol {
        DvTrunkProtocol::CapacityPlus => "Capacity Plus",
        DvTrunkProtocol::HyteraXpt => "Hytera XPT",
        DvTrunkProtocol::TierThree => "Tier III",
    });
    let role = f.control_channel.map(|control| {
        if control {
            "control channel"
        } else {
            "traffic channel"
        }
    });
    joined(system.into_iter().chain(role).map(str::to_owned))
}

fn dv_channel_definition(f: &DvFrame) -> Option<FieldValue> {
    let definition = f.channel_definition.as_ref()?;
    text_of(format!(
        "LCN {} · TX {} · RX {}",
        definition.channel,
        hertz(definition.tx_hz as f64),
        hertz(definition.rx_hz as f64)
    ))
}

fn slot_activity(f: &DvFrame) -> String {
    f.slot_activity
        .iter()
        .map(|item| {
            let hash = item
                .destination_hash
                .map(|hash| format!(" (hash {})", hex(hash, 2)))
                .unwrap_or_default();
            format!("TS{} {}{hash}", item.slot, item.activity)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn dv_summary(f: &DvFrame) -> String {
    let mut parts = vec![f.mode.label().to_owned()];
    if let Some(slot) = f.slot {
        parts.push(format!("TS{slot}"));
    }
    parts.extend(color_code(f));
    parts.extend(f.parties());
    parts.extend(f.talker_alias.clone());
    parts.extend(f.vendor.map(|vendor| vendor.label().to_owned()));
    parts.extend(f.via.as_ref().map(|via| format!("via {via}")));
    parts.extend(f.opcode.clone());
    if f.encrypted == Some(true) {
        parts.push(match (f.algorithm_id, f.key_id) {
            (Some(algorithm), Some(key)) => {
                format!("encrypted ALG {algorithm:02X} KID {key:04X}")
            }
            _ => "encrypted".to_owned(),
        });
    }
    parts.extend(f.text.clone());
    joined(parts)
}

impl EventPayload for VoiceCall {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: self.source.map(|id| id.to_string()),
            voice: Some(VoiceFacts {
                source: self.source,
                destination: self.destination,
                encrypted: Some(self.encrypted),
                emergency: Some(self.emergency),
            }),
            duration_ms: Some(self.duration_ms),
            clip: self.audio.clone(),
            fields: call_fields(self),
            ..Presentation::of(call_summary(self))
        }
    }

    fn strip_clip(&mut self) {
        self.audio = None;
    }
}

fn call_fields(c: &VoiceCall) -> Vec<Field> {
    let destination = c.destination.map(|id| match c.group_call {
        Some(false) => format!("radio {id}"),
        _ => format!("talkgroup {id}"),
    });
    fields(&[
        ("Mode", text_of(c.mode_label())),
        ("Destination", text(destination.as_deref())),
        ("Source", c.source.and_then(text_of)),
        ("Timeslot", count(c.slot)),
        ("Colour code", count(c.color_code)),
        ("Duration", seconds(c.duration_ms, 1)),
        ("Started", Some(FieldValue::Time(c.started_at.clone()))),
        ("Emergency", only_if(c.emergency)),
        ("Encrypted", only_if(c.encrypted)),
        ("Audio", text(c.audio_error.as_deref())),
    ])
}

fn seconds(ms: u64, digits: u8) -> Option<FieldValue> {
    number(Some(ms as f64 / 1_000.0), Unit::Seconds, digits)
}

fn call_summary(c: &VoiceCall) -> String {
    let mut parts = vec![c.mode_label()];
    if let Some(id) = c.destination {
        parts.push(match c.group_call {
            Some(true) => format!("talkgroup {id}"),
            Some(false) => format!("radio {id}"),
            None => format!("to {id}"),
        });
    }
    parts.extend(c.source.map(|id| format!("from {id}")));
    parts.extend(c.slot.map(|slot| format!("TS{slot}")));
    parts.extend(c.color_code.map(|cc| format!("CC {cc}")));
    parts.push(format!("{:.1} s", c.duration_ms as f64 / 1_000.0));
    if c.emergency {
        parts.push("emergency".to_owned());
    }
    if c.encrypted {
        parts.push("encrypted".to_owned());
    }
    joined(parts)
}

impl EventPayload for Transmission {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            duration_ms: Some(self.duration_ms),
            clip: self.audio.clone(),
            fields: transmission_fields(self),
            ..Presentation::of(self.summary())
        }
    }

    fn strip_clip(&mut self) {
        self.audio = None;
    }
}

fn transmission_fields(t: &Transmission) -> Vec<Field> {
    let decoder = t.decoder.as_deref().map(|decoder| {
        let how = if t.decoder_confirmed {
            "confirmed"
        } else {
            "estimated"
        };
        format!("{} ({how})", decoder.to_uppercase())
    });
    fields(&[
        ("Transmission", text_of(t.id)),
        ("State", text_of(t.state.label())),
        (
            "Frequency",
            number(Some(t.signal.frequency_hz), Unit::Hz, 0),
        ),
        (
            "Bandwidth",
            number(Some(t.signal.bandwidth_hz), Unit::Hz, 0),
        ),
        ("Modulation", text_of(modulation_label(&t.signal))),
        ("Decoder", text(decoder.as_deref())),
        ("Confidence", percent(t.signal.confidence)),
        ("SNR", number(Some(t.signal.snr_db), Unit::Db, 1)),
        ("Duration", seconds(t.duration_ms, 2)),
        ("Error", text(t.error.as_deref())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dv(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<DvFrame>(json)
            .unwrap()
            .present(EventKind::Dv)
    }

    #[test]
    fn dv_tells_a_said_no_from_silence() {
        let said = dv(serde_json::json!({
            "mode": "dmr", "kind": "header", "errors_corrected": 0, "encrypted": false,
        }));
        assert_eq!(said.field("Encrypted"), flag(Some(false)).as_ref());
        let silent =
            dv(serde_json::json!({ "mode": "dmr", "kind": "header", "errors_corrected": 0 }));
        assert_eq!(silent.field("Encrypted"), None);
        assert_eq!(silent.labels(), ["Mode", "Frame"]);
    }

    #[test]
    fn dv_exposes_dmr_metadata_and_keeps_packet_data() {
        let view = dv(serde_json::json!({
            "mode": "dmr", "kind": "control", "errors_corrected": 2, "vendor": "hytera",
            "manufacturer_id": 8, "talker_alias": "Dispatcher", "lat": 52.52, "lon": 13.405,
            "position_error_m": 20, "channel": 407, "emergency": true, "algorithm_id": 5,
            "key_id": 42, "message_indicator": "001122334455667788",
            "slot_activity": [{ "slot": 2, "activity": "group voice", "destination_hash": 171 }],
            "data": "A1B2C3",
        }));
        assert_eq!(view.field("Vendor"), text_of("Hytera (0x08)").as_ref());
        assert_eq!(view.field("Talker alias"), text_of("Dispatcher").as_ref());
        assert_eq!(
            view.field("Position error"),
            number(Some(20), Unit::Metres, 0).as_ref()
        );
        assert_eq!(view.field("Channel"), count(Some(407)).as_ref());
        assert_eq!(view.field("Emergency"), flag(Some(true)).as_ref());
        assert_eq!(
            view.field("Slot activity"),
            text_of("TS2 group voice (hash 0xAB)").as_ref()
        );
        assert_eq!(view.field("Algorithm"), text_of("0x05").as_ref());
        assert_eq!(view.field("Key ID"), text_of("0x002A").as_ref());
        assert_eq!(view.field("Repaired"), count(Some(2)).as_ref());
        assert_eq!(view.field("Data"), text_of("A1B2C3").as_ref());
    }

    #[test]
    fn a_tetra_cell_is_named_by_country_network_and_area() {
        let view = dv(serde_json::json!({
            "mode": "tetra", "kind": "control", "errors_corrected": 0, "system_id": 250,
            "network_id": 13, "site_id": 1052, "color_code": 44,
        }));
        assert_eq!(view.field("MCC"), text_of(250).as_ref());
        assert_eq!(view.field("MNC"), text_of(13).as_ref());
        assert_eq!(view.field("Location area"), text_of(1052).as_ref());
        assert_eq!(view.field("System ID"), None);
    }

    #[test]
    fn trunking_names_the_side_and_the_checksum() {
        let control = dv(serde_json::json!({
            "mode": "dmr", "kind": "control", "errors_corrected": 0,
            "trunk_protocol": "tier_three", "control_channel": true, "crc_verified": true,
        }));
        assert_eq!(
            control.field("Trunking"),
            text_of("Tier III · control channel").as_ref()
        );
        assert_eq!(control.field("Checksum"), text_of("verified").as_ref());
        let traffic = dv(serde_json::json!({
            "mode": "dmr", "kind": "control", "errors_corrected": 0,
            "trunk_protocol": "tier_three", "control_channel": false, "crc_verified": false,
        }));
        assert_eq!(
            traffic.field("Trunking"),
            text_of("Tier III · traffic channel").as_ref()
        );
        assert_eq!(
            traffic.field("Checksum"),
            text_of("not verified: read on error correction alone").as_ref()
        );
        let plain =
            dv(serde_json::json!({ "mode": "dmr", "kind": "voice", "errors_corrected": 0 }));
        assert_eq!(plain.field("Trunking"), None);
        assert_eq!(plain.field("Checksum"), None);
    }

    #[test]
    fn a_call_lists_its_parties_and_timing() {
        let view = serde_json::from_value::<VoiceCall>(serde_json::json!({
            "id": 1, "node": "n", "started_at": "2026-01-01T00:00:00Z",
            "ended_at": "2026-01-01T00:00:04Z", "duration_ms": 4200, "device_set": 0,
            "channel": 0, "freq_hz": 438e6, "mode": "dmr", "slot": 1, "destination": 9,
            "group_call": false, "source": 2621, "encrypted": false, "emergency": true,
        }))
        .unwrap()
        .present(EventKind::Call);
        assert_eq!(view.field("Mode"), text_of("DMR").as_ref());
        assert_eq!(view.field("Destination"), text_of("radio 9").as_ref());
        assert_eq!(
            view.field("Duration"),
            number(Some(4.2), Unit::Seconds, 1).as_ref()
        );
        assert_eq!(
            view.field("Started"),
            Some(&FieldValue::Time("2026-01-01T00:00:00Z".to_owned()))
        );
        assert_eq!(view.field("Emergency"), only_if(true).as_ref());
        assert_eq!(view.field("Encrypted"), None);
    }

    #[test]
    fn a_transmission_lists_its_signal_and_decoder() {
        let mut view = crate::decode::fixtures::fixture(EventKind::Transmission).present();
        assert_eq!(view.field("State"), text_of("completed").as_ref());
        assert_eq!(
            view.field("Frequency"),
            number(Some(145_500_000.0), Unit::Hz, 0).as_ref()
        );
        assert_eq!(view.field("Confidence"), percent(0.9).as_ref());
        assert_eq!(
            view.field("Duration"),
            number(Some(1.0), Unit::Seconds, 2).as_ref()
        );
        assert_eq!(view.field("Decoder"), None);
        let crate::DecoderEvent::Transmission(mut heard) =
            crate::decode::fixtures::fixture(EventKind::Transmission)
        else {
            panic!("transmission fixture");
        };
        heard.decoder = Some("pocsag".to_owned());
        heard.decoder_confirmed = true;
        view = heard.present(EventKind::Transmission);
        assert_eq!(
            view.field("Decoder"),
            text_of("POCSAG (confirmed)").as_ref()
        );
    }

    #[test]
    fn tone_and_scrambler_show_what_was_heard() {
        let tone = ToneSquelchStatus {
            ctcss_hz: Some(88.5),
            dcs_code: None,
            open: true,
        }
        .present(EventKind::Tone);
        assert_eq!(tone.labels(), ["CTCSS", "Audio"]);
        assert_eq!(tone.field("Audio"), text_of("open").as_ref());
        let dcs = ToneSquelchStatus {
            ctcss_hz: None,
            dcs_code: Some(23),
            open: false,
        }
        .present(EventKind::Tone);
        assert_eq!(dcs.field("DCS"), text_of("023").as_ref());
        let none = ScramblerStatus {
            inversion_hz: None,
            confidence: 0.2,
        }
        .present(EventKind::Scrambler);
        assert!(none.fields.is_empty());
        let found = ScramblerStatus {
            inversion_hz: Some(3_300.0),
            confidence: 0.9,
        }
        .present(EventKind::Scrambler);
        assert_eq!(found.labels(), ["Inversion", "Confidence"]);
    }
}
