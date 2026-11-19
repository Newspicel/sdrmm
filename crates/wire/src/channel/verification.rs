use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// How far a decoder is proven. The catalogs in the docs and on the site are built from it.
///
/// A decoder that does not say, such as a plugin built before the field, is `Unverified`: no
/// claim is made for it.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    OnAir,
    ReferenceFixture,
    OwnModulator,
    Experimental,
    #[default]
    Unverified,
}

impl Verification {
    pub const ALL: [Self; 5] = [
        Self::OnAir,
        Self::ReferenceFixture,
        Self::OwnModulator,
        Self::Experimental,
        Self::Unverified,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OnAir => "Tested on air",
            Self::ReferenceFixture => "Reference fixture",
            Self::OwnModulator => "Own modulator",
            Self::Experimental => "Experimental",
            Self::Unverified => "Unverified",
        }
    }

    #[must_use]
    pub const fn meaning(self) -> &'static str {
        match self {
            Self::OnAir => "Verified on a real off-air signal, live or recorded",
            Self::ReferenceFixture => {
                "Verified on another implementation's waveform or published vectors"
            }
            Self::OwnModulator => "Verified only against this project's own modulator",
            Self::Experimental => "Works with the limits below",
            Self::Unverified => "Makes no claim",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelDescriptor;

    #[test]
    fn a_descriptor_carries_its_verification_by_name() {
        let descriptor = ChannelDescriptor {
            type_id: "probe".to_owned(),
            verification: Verification::ReferenceFixture,
            ..ChannelDescriptor::default()
        };
        let json = serde_json::to_value(&descriptor).expect("serialize");
        assert_eq!(json["verification"], "reference_fixture");
        let back: ChannelDescriptor = serde_json::from_value(json).expect("parse");
        assert_eq!(back.verification, Verification::ReferenceFixture);
    }

    #[test]
    fn a_descriptor_that_does_not_say_makes_no_claim() {
        let json = serde_json::json!({
            "type_id": "plugin",
            "name": "Plugin",
            "bandwidth_hz": 12_500.0,
            "input_rate_hz": 48_000.0,
        });
        let back: ChannelDescriptor = serde_json::from_value(json).expect("parse");
        assert_eq!(back.verification, Verification::Unverified);
    }

    #[test]
    fn every_level_has_a_label_and_a_meaning() {
        for level in Verification::ALL {
            assert!(!level.label().is_empty());
            assert!(!level.meaning().is_empty());
        }
        assert_eq!(
            serde_json::to_value(Verification::OnAir).expect("serialize"),
            "on_air"
        );
    }
}
