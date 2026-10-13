use super::{
    EventPayload, Field, FieldValue, Presentation, Unit, count, fields, flag, joined, number, text,
    text_of,
};
use crate::decode::{DectCipherState, DectFrame, DectIdentity, DectSecurity, DectSide, EventKind};

const DECT_CARRIERS: u16 = 10;

impl EventPayload for DectFrame {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: self.identity.as_ref().map(|id| id.rfpi.clone()),
            fields: dect_fields(self),
            ..Presentation::of(dect_summary(self))
        }
    }
}

fn dect_fields(f: &DectFrame) -> Vec<Field> {
    let side = match f.side {
        DectSide::Rfp => "base station",
        DectSide::Pp => "handset",
    };
    let mut shown = fields(&[("Side", text_of(side))]);
    shown.extend(f.identity.as_ref().map(identity_fields).unwrap_or_default());
    shown.extend(fields(&[
        ("Carrier", count(f.carrier)),
        ("Frequency", number(f.carrier_hz, Unit::Hz, 0)),
        ("Slot pair", count(f.slot_pair)),
        ("Transceivers", count(f.transceivers)),
        ("Carriers available", f.rf_carriers.and_then(carriers)),
        ("Scan carrier", count(f.pscn)),
        ("Multiframe", count(f.multiframe)),
    ]));
    shown.extend(security_fields(&f.security));
    shown.extend(fields(&[
        ("FMID", f.fmid.and_then(|id| text_of(format!("{id:04X}")))),
        ("PMID", f.pmid.and_then(|id| text_of(format!("{id:05X}")))),
        ("Handsets", text(Some(&handsets(f)))),
    ]));
    shown.extend(voice_fields(f));
    shown.extend(fields(&[
        ("Bursts", count(Some(f.bursts))),
        ("Level", number(Some(f.level_dbfs), Unit::Dbfs, 1)),
        ("Capabilities", text(Some(&capabilities(f)))),
    ]));
    shown
}

fn identity_fields(id: &DectIdentity) -> Vec<Field> {
    let cell = id
        .multicell
        .map(|multi| if multi { "multi-cell" } else { "single cell" });
    fields(&[
        ("RFPI", text(Some(&id.rfpi))),
        ("Access class", text_of(id.arc.label())),
        ("PARI", text(Some(&id.pari))),
        (
            "Manufacturer",
            id.emc.and_then(|code| text_of(format!("{code:04X}"))),
        ),
        (
            "Installer",
            id.eic.and_then(|code| text_of(format!("{code:04X}"))),
        ),
        (
            "Operator",
            id.poc.and_then(|code| text_of(format!("{code:04X}"))),
        ),
        (
            "GSM/UMTS operator",
            id.gop.and_then(|code| text_of(format!("{code:05X}"))),
        ),
        ("Fixed part", count(id.fpn)),
        ("Fixed part sub", count(id.fps)),
        ("Radio fixed part", count(Some(id.rpn))),
        ("Cell", cell.and_then(text_of)),
        ("SARI list", flag(Some(id.sari_available))),
    ])
}

fn security_fields(security: &DectSecurity) -> Vec<Field> {
    fields(&[
        ("Authentication", flag(security.authentication_supported)),
        ("Ciphering", flag(security.ciphering_supported)),
        ("Encryption", text_of(security.cipher_state.label())),
        ("DSAA2", flag(security.dsaa2_supported)),
        ("DSC2", flag(security.dsc2_supported)),
        (
            "Last cipher command",
            text(security.last_command.as_deref()),
        ),
        ("Cipher key index", count(security.cipher_key_index)),
    ])
}

fn voice_fields(f: &DectFrame) -> Vec<Field> {
    let Some(voice) = f.voice else {
        return Vec::new();
    };
    let nonzero = |value: u32| count(Some(value).filter(|value| *value > 0));
    fields(&[
        (
            "Voice",
            text_of(if voice.playing { "playing" } else { "muted" }),
        ),
        ("Voice frames", count(Some(voice.frames))),
        ("Voice X-CRC errors", nonzero(voice.x_crc_errors)),
        ("Voice before sync", nonzero(voice.unsynced)),
        ("Voice encrypted", nonzero(voice.encrypted)),
        ("Voice late", nonzero(voice.late)),
    ])
}

fn carriers(mask: u16) -> Option<FieldValue> {
    let on = (0..DECT_CARRIERS)
        .filter(|carrier| mask >> (DECT_CARRIERS - 1 - carrier) & 1 == 1)
        .map(|carrier| carrier.to_string())
        .collect::<Vec<_>>();
    text(Some(&on.join(", ")))
}

fn handsets(f: &DectFrame) -> String {
    f.handsets
        .iter()
        .map(|id| format!("{id:05X}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn capabilities(f: &DectFrame) -> String {
    f.capabilities
        .iter()
        .map(|capability| capability.label())
        .collect::<Vec<_>>()
        .join("\n")
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

fn dect_summary(f: &DectFrame) -> String {
    let mut parts = Vec::new();
    if let Some(identity) = &f.identity {
        parts.push(format!("RFPI {}", identity.rfpi));
        parts.push(format!("class {}", identity.arc.label()));
    } else {
        parts.push(f.side.label().to_owned());
    }
    parts.extend(f.carrier.map(|carrier| format!("carrier {carrier}")));
    parts.extend(f.slot_pair.map(|slot| format!("slot {slot}")));
    parts.extend(security_parts(&f.security));
    if f.voice.is_some_and(|voice| voice.playing) {
        parts.push("voice".to_owned());
    }
    joined(parts)
}

fn security_parts(security: &DectSecurity) -> Vec<String> {
    let mut parts = Vec::new();
    if let (Some(auth), Some(cipher)) = (
        security.authentication_supported,
        security.ciphering_supported,
    ) {
        parts.push(format!("auth {} · cipher {}", yes_no(auth), yes_no(cipher)));
    }
    let dsaa2 = security.dsaa2_supported == Some(true);
    let dsc2 = security.dsc2_supported == Some(true);
    if dsaa2 || dsc2 {
        parts.push(format!("DSAA2 {} · DSC2 {}", yes_no(dsaa2), yes_no(dsc2)));
    }
    if security.cipher_state != DectCipherState::Clear {
        parts.push(security.cipher_state.label().to_owned());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bearer_shows_dsaa2_dsc2_and_voice_counters() {
        let view = serde_json::from_value::<DectFrame>(serde_json::json!({
            "side": "rfp", "update": "voice",
            "capabilities": ["dsaa2", "dsc2", "ule_phase1_revised"],
            "security": {
                "cipher_state": "clear", "encryption_events": 0,
                "dsaa2_supported": true, "dsc2_supported": false,
            },
            "voice": { "frames": 120, "playing": true, "x_crc_errors": 3, "unsynced": 0,
                "encrypted": 0, "late": 0 },
            "extended_carriers": false, "bursts": 140, "level_dbfs": -40.0,
        }))
        .unwrap()
        .present(EventKind::Dect);
        assert_eq!(view.field("DSAA2"), flag(Some(true)).as_ref());
        assert_eq!(view.field("DSC2"), flag(Some(false)).as_ref());
        assert_eq!(view.field("Voice"), text_of("playing").as_ref());
        assert_eq!(view.field("Voice frames"), count(Some(120)).as_ref());
        assert_eq!(view.field("Voice X-CRC errors"), count(Some(3)).as_ref());
        assert_eq!(view.field("Voice before sync"), None);
        assert_eq!(
            view.field("Capabilities"),
            text_of("DSAA2 authentication\nDSC2 ciphering\nULE phase 1 v1.2").as_ref()
        );
    }

    #[test]
    fn a_base_station_names_its_identity_and_carriers() {
        let view = serde_json::from_value::<DectFrame>(serde_json::json!({
            "side": "rfp", "update": "identity",
            "identity": { "rfpi": "0123456789", "pari": "01234567", "arc": "a",
                "sari_available": false, "rpn": 9, "emc": 291 },
            "rf_carriers": 0b10_0000_0001,
            "security": { "cipher_state": "active", "encryption_events": 1 },
            "bursts": 4, "level_dbfs": -61.25,
        }))
        .unwrap()
        .present(EventKind::Dect);
        assert_eq!(view.field("RFPI"), text_of("0123456789").as_ref());
        assert_eq!(
            view.field("Access class"),
            text_of("A residential / small PBX").as_ref()
        );
        assert_eq!(view.field("Manufacturer"), text_of("0123").as_ref());
        assert_eq!(view.field("Carriers available"), text_of("0, 9").as_ref());
        assert_eq!(
            view.field("Encryption"),
            text_of("encryption active").as_ref()
        );
        assert_eq!(
            view.field("Level"),
            number(Some(-61.25_f32), Unit::Dbfs, 1).as_ref()
        );
    }
}
