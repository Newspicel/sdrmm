use crate::default_true;
use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::picture_fields;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AptParams {
    #[serde(default = "default_true")]
    pub keep_partial: bool,
}

impl Default for AptParams {
    fn default() -> Self {
        Self { keep_partial: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvhrrChannel {
    Ch1,
    Ch2,
    Ch3a,
    Ch3b,
    Ch4,
    Ch5,
}

impl AvhrrChannel {
    pub const ALL: [Self; 6] = [
        Self::Ch1,
        Self::Ch2,
        Self::Ch3a,
        Self::Ch3b,
        Self::Ch4,
        Self::Ch5,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ch1 => "1",
            Self::Ch2 => "2",
            Self::Ch3a => "3A",
            Self::Ch3b => "3B",
            Self::Ch4 => "4",
            Self::Ch5 => "5",
        }
    }

    #[must_use]
    pub const fn wedge(self) -> u8 {
        match self {
            Self::Ch1 => 1,
            Self::Ch2 => 2,
            Self::Ch3a => 3,
            Self::Ch4 => 4,
            Self::Ch5 => 5,
            Self::Ch3b => 6,
        }
    }

    #[must_use]
    pub fn from_wedge(wedge: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|channel| channel.wedge() == wedge)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AptImage {
    pub seq: u32,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_a: Option<AvhrrChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_b: Option<AvhrrChannel>,
}

fn apt_summary(p: &AptImage) -> String {
    let mut parts = vec!["APT".to_owned()];
    if let (Some(a), Some(b)) = (p.channel_a, p.channel_b) {
        parts.push(format!("ch {}/{}", a.label(), b.label()));
    }
    parts.push(if p.complete {
        format!("{} lines in {} s", p.lines, p.duration_ms / 1_000)
    } else {
        format!("{} lines, cut short", p.lines)
    });
    parts.join(" · ")
}

impl EventPayload for AptImage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: picture_fields(
                &[("channels", "Channels", apt_channels(self))],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(apt_summary(self))
        }
    }
}

fn apt_channels(p: &AptImage) -> Option<FieldValue> {
    if p.channel_a.is_none() && p.channel_b.is_none() {
        return None;
    }
    let label = |channel: Option<AvhrrChannel>| channel.map_or("?", AvhrrChannel::label);
    text_of(format!(
        "ch {} + {}",
        label(p.channel_a),
        label(p.channel_b)
    ))
}

pub(crate) fn apt_kind() -> KindSpec {
    KindSpec::of::<AptImage>("NOAA APT", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .view(KindView::Picture)
        .build()
}

impl EventContract for AptImage {
    const KIND: EventKind = EventKind::named("apt");
}

impl sdrmm_wire::DecoderSettings for AptParams {
    const TYPE_ID: &'static str = "apt";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::present::Unit;
    use sdrmm_wire::present::number;

    #[test]
    fn avhrr_wedges_name_their_channel() {
        for channel in AvhrrChannel::ALL {
            assert_eq!(AvhrrChannel::from_wedge(channel.wedge()), Some(channel));
        }
        assert_eq!(AvhrrChannel::from_wedge(7), None);
    }

    #[test]
    fn apt_names_its_avhrr_channels() {
        let view = AptImage {
            seq: 2,
            lines: 900,
            complete: false,
            duration_ms: 450_000,
            channel_a: Some(AvhrrChannel::Ch2),
            channel_b: Some(AvhrrChannel::Ch4),
        }
        .present(&EventKind::named("apt"));
        assert_eq!(view.labels(), ["Channels", "Lines", "Complete", "Took"]);
        assert_eq!(view.field("Channels"), text_of("ch 2 + 4").as_ref());
        assert_eq!(
            view.field("Took"),
            number(Some(450.0), Unit::Seconds, 1).as_ref()
        );
        assert_eq!(view.entity, None);
    }
}
