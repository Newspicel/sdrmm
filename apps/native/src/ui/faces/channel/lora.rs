use sdrmm_wire::lora::{
    LoraBandwidth, LoraCodingRate, LoraImplicitHeader, LoraIq, LoraKey, LoraParams, LoraProtocol,
    LoraSpreadingFactor,
};

use super::tables::{Options, Titled};

pub const LORA_BANDWIDTHS: Options<LoraBandwidth> = &[
    (LoraBandwidth::Khz7_8, "7.8 kHz"),
    (LoraBandwidth::Khz10_4, "10.4 kHz"),
    (LoraBandwidth::Khz15_6, "15.6 kHz"),
    (LoraBandwidth::Khz20_8, "20.8 kHz"),
    (LoraBandwidth::Khz31_25, "31.25 kHz"),
    (LoraBandwidth::Khz41_7, "41.7 kHz"),
    (LoraBandwidth::Khz62_5, "62.5 kHz"),
    (LoraBandwidth::Khz125, "125 kHz"),
    (LoraBandwidth::Khz250, "250 kHz"),
    (LoraBandwidth::Khz500, "500 kHz"),
];
pub const LORA_SPREADING_FACTORS: Options<LoraSpreadingFactor> = &[
    (LoraSpreadingFactor::All, "All"),
    (LoraSpreadingFactor::Sf7, "SF7"),
    (LoraSpreadingFactor::Sf8, "SF8"),
    (LoraSpreadingFactor::Sf9, "SF9"),
    (LoraSpreadingFactor::Sf10, "SF10"),
    (LoraSpreadingFactor::Sf11, "SF11"),
    (LoraSpreadingFactor::Sf12, "SF12"),
];
pub const LORA_IQ: Titled<LoraIq> = &[
    (LoraIq::Normal, "Normal", None),
    (LoraIq::Inverted, "Inverted", Some("LoRaWAN downlinks")),
    (LoraIq::Both, "Both", None),
];
pub const LORA_PROTOCOLS: Options<LoraProtocol> = &[
    (LoraProtocol::Auto, "Auto"),
    (LoraProtocol::Raw, "LoRa"),
    (LoraProtocol::Lorawan, "LoRaWAN"),
    (LoraProtocol::Meshtastic, "Meshtastic"),
    (LoraProtocol::Meshcore, "MeshCore"),
];
pub const LORA_CODING_RATES: Options<LoraCodingRate> = &[
    (LoraCodingRate::Cr45, "4/5"),
    (LoraCodingRate::Cr46, "4/6"),
    (LoraCodingRate::Cr47, "4/7"),
    (LoraCodingRate::Cr48, "4/8"),
];
pub const KEYS_TITLE: &str = "Extra keys; Meshtastic defaults and MeshCore Public are built in";
pub const IMPLICIT_LENGTH: (f64, f64) = (1.0, 255.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoraModem {
    pub bandwidth: LoraBandwidth,
    pub spreading_factor: LoraSpreadingFactor,
    pub protocol: LoraProtocol,
    pub iq: LoraIq,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoraPreset {
    pub label: &'static str,
    pub title: &'static str,
    pub modem: LoraModem,
}

const fn meshtastic(
    label: &'static str,
    bandwidth: LoraBandwidth,
    spreading_factor: LoraSpreadingFactor,
) -> LoraPreset {
    LoraPreset {
        label,
        title: "Meshtastic modem preset",
        modem: LoraModem {
            bandwidth,
            spreading_factor,
            protocol: LoraProtocol::Meshtastic,
            iq: LoraIq::Normal,
        },
    }
}

const fn meshcore(label: &'static str, spreading_factor: LoraSpreadingFactor) -> LoraPreset {
    LoraPreset {
        label,
        title: "MeshCore narrow preset",
        modem: LoraModem {
            bandwidth: LoraBandwidth::Khz62_5,
            spreading_factor,
            protocol: LoraProtocol::Meshcore,
            iq: LoraIq::Normal,
        },
    }
}

pub const LORA_PRESETS: [LoraPreset; 11] = [
    meshtastic("LongFast", LoraBandwidth::Khz250, LoraSpreadingFactor::Sf11),
    meshtastic(
        "LongModerate",
        LoraBandwidth::Khz125,
        LoraSpreadingFactor::Sf11,
    ),
    meshtastic("LongSlow", LoraBandwidth::Khz125, LoraSpreadingFactor::Sf12),
    meshtastic(
        "MediumSlow",
        LoraBandwidth::Khz250,
        LoraSpreadingFactor::Sf10,
    ),
    meshtastic(
        "MediumFast",
        LoraBandwidth::Khz250,
        LoraSpreadingFactor::Sf9,
    ),
    meshtastic("ShortSlow", LoraBandwidth::Khz250, LoraSpreadingFactor::Sf8),
    meshtastic("ShortFast", LoraBandwidth::Khz250, LoraSpreadingFactor::Sf7),
    meshtastic(
        "ShortTurbo",
        LoraBandwidth::Khz500,
        LoraSpreadingFactor::Sf7,
    ),
    meshcore("MeshCore EU", LoraSpreadingFactor::Sf8),
    meshcore("MeshCore US", LoraSpreadingFactor::Sf7),
    LoraPreset {
        label: "LoRaWAN",
        title: "Uplinks and downlinks on one 125 kHz channel",
        modem: LoraModem {
            bandwidth: LoraBandwidth::Khz125,
            spreading_factor: LoraSpreadingFactor::All,
            protocol: LoraProtocol::Lorawan,
            iq: LoraIq::Both,
        },
    },
];

#[must_use]
pub fn modem_of(params: &LoraParams) -> LoraModem {
    LoraModem {
        bandwidth: params.bandwidth,
        spreading_factor: params.spreading_factor,
        protocol: params.protocol,
        iq: params.iq,
    }
}

#[must_use]
pub fn preset_of(params: &LoraParams) -> Option<usize> {
    let modem = modem_of(params);
    LORA_PRESETS.iter().position(|preset| preset.modem == modem)
}

pub fn apply_preset(params: &mut LoraParams, index: usize) {
    if let Some(preset) = LORA_PRESETS.get(index) {
        params.bandwidth = preset.modem.bandwidth;
        params.spreading_factor = preset.modem.spreading_factor;
        params.protocol = preset.modem.protocol;
        params.iq = preset.modem.iq;
    }
}

#[must_use]
pub fn preset_options() -> Vec<(Option<usize>, String)> {
    LORA_PRESETS
        .iter()
        .enumerate()
        .map(|(index, preset)| (Some(index), preset.label.to_owned()))
        .chain(std::iter::once((None, String::from("Custom"))))
        .collect()
}

#[must_use]
pub fn with_implicit_length(
    header: Option<LoraImplicitHeader>,
    length: Option<f64>,
) -> Option<LoraImplicitHeader> {
    let length = length?.round().clamp(IMPLICIT_LENGTH.0, IMPLICIT_LENGTH.1) as u8;
    Some(match header {
        Some(held) => LoraImplicitHeader { length, ..held },
        None => LoraImplicitHeader {
            length,
            coding_rate: LoraCodingRate::Cr45,
            crc: true,
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Meshtastic,
    Meshcore,
    LorawanSession,
    LorawanAppKey,
}

pub const KEY_KINDS: [(KeyKind, &str, &str); 4] = [
    (
        KeyKind::Meshtastic,
        "Meshtastic",
        "Channel name and base64 PSK",
    ),
    (KeyKind::Meshcore, "MeshCore", "Channel name and hex secret"),
    (
        KeyKind::LorawanSession,
        "ABP",
        "LoRaWAN DevAddr, NwkSKey and AppSKey",
    ),
    (KeyKind::LorawanAppKey, "OTAA", "LoRaWAN AppKey"),
];

#[must_use]
pub fn kind_of(key: &LoraKey) -> KeyKind {
    match key {
        LoraKey::MeshtasticChannel { .. } => KeyKind::Meshtastic,
        LoraKey::MeshcoreChannel { .. } => KeyKind::Meshcore,
        LoraKey::LorawanSession { .. } => KeyKind::LorawanSession,
        LoraKey::LorawanAppKey { .. } => KeyKind::LorawanAppKey,
    }
}

#[must_use]
pub fn kind_label(kind: KeyKind) -> (&'static str, &'static str) {
    KEY_KINDS
        .iter()
        .find(|(held, _, _)| *held == kind)
        .map_or(("", ""), |(_, label, title)| (*label, *title))
}

#[must_use]
pub fn key_fields(kind: KeyKind) -> &'static [&'static str] {
    match kind {
        KeyKind::Meshtastic => &["Channel", "PSK base64"],
        KeyKind::Meshcore => &["Channel or #tag", "Secret hex"],
        KeyKind::LorawanSession => &["DevAddr", "NwkSKey", "AppSKey"],
        KeyKind::LorawanAppKey => &["AppKey"],
    }
}

#[must_use]
pub fn blank_key(kind: KeyKind) -> LoraKey {
    match kind {
        KeyKind::Meshtastic => LoraKey::MeshtasticChannel {
            name: String::new(),
            psk: String::new(),
        },
        KeyKind::Meshcore => LoraKey::MeshcoreChannel {
            name: String::new(),
            secret: String::new(),
        },
        KeyKind::LorawanSession => LoraKey::LorawanSession {
            dev_addr: String::new(),
            nwk_s_key: String::new(),
            app_s_key: String::new(),
        },
        KeyKind::LorawanAppKey => LoraKey::LorawanAppKey {
            app_key: String::new(),
        },
    }
}

fn texts_mut(key: &mut LoraKey) -> Vec<&mut String> {
    match key {
        LoraKey::MeshtasticChannel { name, psk } => vec![name, psk],
        LoraKey::MeshcoreChannel { name, secret } => vec![name, secret],
        LoraKey::LorawanSession {
            dev_addr,
            nwk_s_key,
            app_s_key,
        } => vec![dev_addr, nwk_s_key, app_s_key],
        LoraKey::LorawanAppKey { app_key } => vec![app_key],
    }
}

#[must_use]
pub fn key_text(key: &LoraKey, field: usize) -> String {
    let mut held = key.clone();
    texts_mut(&mut held)
        .get(field)
        .map(|text| (**text).clone())
        .unwrap_or_default()
}

#[must_use]
pub fn with_key_text(key: &LoraKey, field: usize, text: &str) -> LoraKey {
    let mut next = key.clone();
    if let Some(slot) = texts_mut(&mut next).into_iter().nth(field) {
        text.clone_into(slot);
    }
    next
}

#[must_use]
pub fn key_complete(key: &LoraKey) -> bool {
    match key {
        LoraKey::MeshtasticChannel { name, .. } => !name.is_empty(),
        LoraKey::MeshcoreChannel { name, secret } => {
            if secret.is_empty() {
                name.starts_with('#') && name.len() > 1
            } else {
                !name.is_empty()
            }
        }
        LoraKey::LorawanSession {
            dev_addr,
            nwk_s_key,
            app_s_key,
        } => !dev_addr.is_empty() && !nwk_s_key.is_empty() && !app_s_key.is_empty(),
        LoraKey::LorawanAppKey { app_key } => !app_key.is_empty(),
    }
}

#[must_use]
pub fn keys_shown(count: usize) -> String {
    if count == 0 {
        String::from("built-in")
    } else {
        format!("+{count}")
    }
}

#[must_use]
pub fn with_key_at(keys: &[LoraKey], index: usize, key: LoraKey) -> Vec<LoraKey> {
    let mut next = keys.to_vec();
    if let Some(slot) = next.get_mut(index) {
        *slot = key;
    }
    next
}

#[must_use]
pub fn without_key(keys: &[LoraKey], index: usize) -> Vec<LoraKey> {
    keys.iter()
        .enumerate()
        .filter(|(at, _)| *at != index)
        .map(|(_, key)| key.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_is_found_by_its_whole_modem_and_custom_otherwise() {
        let mut params = LoraParams::default();
        assert_eq!(preset_of(&params), None);
        apply_preset(&mut params, 0);
        assert_eq!(params.bandwidth, LoraBandwidth::Khz250);
        assert_eq!(params.spreading_factor, LoraSpreadingFactor::Sf11);
        assert_eq!(params.protocol, LoraProtocol::Meshtastic);
        assert_eq!(preset_of(&params), Some(0));
        params.iq = LoraIq::Inverted;
        assert_eq!(preset_of(&params), None);
        apply_preset(&mut params, 10);
        assert_eq!(preset_of(&params), Some(10));
        assert_eq!(params.iq, LoraIq::Both);
    }

    #[test]
    fn the_preset_list_ends_with_custom() {
        let options = preset_options();
        assert_eq!(options.len(), 12);
        assert_eq!(options[8].1, "MeshCore EU");
        assert_eq!(options[11], (None, String::from("Custom")));
    }

    #[test]
    fn an_implicit_length_keeps_its_rate_and_crc_or_takes_the_defaults() {
        assert_eq!(with_implicit_length(None, None), None);
        let fresh = with_implicit_length(None, Some(12.4)).expect("a header");
        assert_eq!(fresh.length, 12);
        assert_eq!(fresh.coding_rate, LoraCodingRate::Cr45);
        assert!(fresh.crc);
        let held = LoraImplicitHeader {
            length: 3,
            coding_rate: LoraCodingRate::Cr48,
            crc: false,
        };
        let kept = with_implicit_length(Some(held), Some(300.0)).expect("a header");
        assert_eq!(kept.length, 255);
        assert_eq!(kept.coding_rate, LoraCodingRate::Cr48);
        assert!(!kept.crc);
        assert_eq!(with_implicit_length(Some(held), None), None);
    }

    #[test]
    fn a_key_is_complete_once_it_names_what_it_unlocks() {
        assert!(!key_complete(&blank_key(KeyKind::Meshtastic)));
        let named = with_key_text(&blank_key(KeyKind::Meshtastic), 0, "LongFast");
        assert!(key_complete(&named));
        let tag = with_key_text(&blank_key(KeyKind::Meshcore), 0, "#test");
        assert!(key_complete(&tag));
        let bare = with_key_text(&blank_key(KeyKind::Meshcore), 0, "test");
        assert!(!key_complete(&bare));
        assert!(key_complete(&with_key_text(&bare, 1, "00ff")));
        let session = with_key_text(
            &with_key_text(&blank_key(KeyKind::LorawanSession), 0, "26011BDA"),
            1,
            "aa",
        );
        assert!(!key_complete(&session));
        assert!(key_complete(&with_key_text(&session, 2, "bb")));
        assert!(key_complete(&with_key_text(
            &blank_key(KeyKind::LorawanAppKey),
            0,
            "cc"
        )));
    }

    #[test]
    fn key_fields_read_and_write_in_their_order() {
        let key = with_key_text(&blank_key(KeyKind::LorawanSession), 2, "app");
        assert_eq!(key_text(&key, 2), "app");
        assert_eq!(key_text(&key, 0), "");
        assert_eq!(key_text(&key, 7), "");
        assert_eq!(with_key_text(&key, 7, "x"), key);
        assert_eq!(key_fields(kind_of(&key)).len(), 3);
        assert_eq!(kind_label(KeyKind::LorawanAppKey).0, "OTAA");
    }

    #[test]
    fn keys_are_edited_and_removed_by_place() {
        let one = with_key_text(&blank_key(KeyKind::LorawanAppKey), 0, "a");
        let two = with_key_text(&blank_key(KeyKind::LorawanAppKey), 0, "b");
        let keys = vec![one.clone(), two.clone()];
        assert_eq!(without_key(&keys, 0), vec![two.clone()]);
        assert_eq!(with_key_at(&keys, 1, one.clone()), vec![one.clone(), one]);
        assert_eq!(keys_shown(0), "built-in");
        assert_eq!(keys_shown(2), "+2");
    }
}
