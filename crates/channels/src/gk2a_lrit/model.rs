use sdrmm_wire::present::{
    EventPayload, Presentation, count, fields, flag, picture_size, text, text_of,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

/// A DES key KMA hands out, by the index the files name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gk2aKey {
    pub index: u16,
    /// Sixteen hex digits.
    pub key: String,
}

impl Gk2aKey {
    /// The eight key bytes, if `key` is sixteen hex digits.
    #[must_use]
    pub fn bytes(&self) -> Option<[u8; 8]> {
        let digits = self.key.trim();
        if digits.len() != 16 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(digits, 16).ok().map(u64::to_be_bytes)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gk2aLritParams {
    #[serde(default)]
    pub keys: Vec<Gk2aKey>,
}

/// One AMI image drawn from its JPEG segments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gk2aLritImage {
    pub seq: u32,
    /// The file name of its first segment.
    pub name: String,
    /// The AMI channel, such as `IR105`.
    pub channel: Option<String>,
    pub width: u16,
    pub lines: u16,
    pub segments: u16,
    pub segments_expected: u16,
    pub complete: bool,
}

fn summary(p: &Gk2aLritImage) -> String {
    let mut parts = vec!["GK-2A".to_owned()];
    parts.extend(p.channel.clone());
    parts.push(format!("{}/{} segments", p.segments, p.segments_expected));
    parts.join(" · ")
}

impl EventPayload for Gk2aLritImage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                ("channel", "Channel", text(self.channel.as_deref())),
                ("size", "Size", picture_size(self.width, self.lines)),
                ("segments", "Segments", count(Some(self.segments))),
                ("complete", "Complete", flag(Some(self.complete))),
                ("name", "File", text_of(&self.name)),
            ]),
            ..Presentation::of(summary(self))
        }
    }
}

pub(crate) fn gk2a_lrit_kind() -> KindSpec {
    KindSpec::of::<Gk2aLritImage>("GK-2A LRIT", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .view(KindView::Picture)
        .build()
}

impl EventContract for Gk2aLritImage {
    const KIND: EventKind = EventKind::named("gk2a_lrit");
}

impl sdrmm_wire::DecoderSettings for Gk2aLritParams {
    const TYPE_ID: &'static str = "gk2a_lrit";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn empty_settings_hold_no_keys() {
        let params: Gk2aLritParams = serde_json::from_str("{}").expect("defaults");
        assert!(params.keys.is_empty());
    }

    #[test]
    fn keys_need_sixteen_hex_digits() {
        let key = |text: &str| Gk2aKey {
            index: 1,
            key: text.to_owned(),
        };
        assert_eq!(
            key("0123456789abcdef").bytes(),
            Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF])
        );
        assert_eq!(key("0123").bytes(), None);
        assert_eq!(key("0123456789abcdeg").bytes(), None);
    }

    #[test]
    fn an_image_names_its_channel() {
        let view = Gk2aLritImage {
            seq: 1,
            name: "IMG_FD_047_IR105_20190713_022006_01.lrit".to_owned(),
            channel: Some("IR105".to_owned()),
            width: 1100,
            lines: 1100,
            segments: 10,
            segments_expected: 10,
            complete: true,
        }
        .present(&EventKind::named("gk2a_lrit"));
        assert_eq!(view.summary, "GK-2A · IR105 · 10/10 segments");
        assert_eq!(view.field("Channel"), text_of("IR105").as_ref());
    }
}
