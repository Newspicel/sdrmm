//! What a voice decoder shares with every other: where its vocoder speaks, the chip that may
//! synthesize the speech, and the privacy keys its calls are deciphered with.

use sdrmm_codec::voice::{
    SpeechSink,
    chip::{self, ChipLink, ChipRate},
};
use sdrmm_wire::{
    ChannelStatus, Control, NumberSpec, SettingField, VoiceKey,
    present::{Field, FieldValue},
};

use crate::{AUDIO_RATE, ChannelError, ChannelOutputs};

const _: () = assert!(sdrmm_codec::OUTPUT_RATE == AUDIO_RATE);

const MAX_KEY_DIGITS: usize = 64;

/// A decoder's outputs as the place its vocoder speaks.
pub struct Speaker<'a>(pub &'a mut ChannelOutputs);

impl SpeechSink for Speaker<'_> {
    fn speak(&mut self, pcm: &[f32]) {
        self.0.audio_pcm.extend_from_slice(pcm);
        self.0.audio_rate = AUDIO_RATE;
    }

    fn vocoder(&mut self, readout: &str) {
        self.0.status = Some(ChannelStatus::readouts(vec![Field::new(
            "vocoder",
            "Vocoder",
            FieldValue::Text(readout.to_owned()),
        )]));
    }
}

/// The chip on `port` for `rate`, or none for software synthesis.
pub fn open_chip(port: Option<&str>, rate: ChipRate) -> Result<Option<ChipLink>, ChannelError> {
    chip::open_chip(port, rate).map_err(|error| match error {
        chip::ChipError::Unsupported => ChannelError::Unsupported(error.to_string()),
        chip::ChipError::Open { .. } => ChannelError::InvalidSettings(error.to_string()),
    })
}

/// `form` with the pick of where voice is synthesized, when this build drives vocoder chips:
/// blip25, or a DVSI chip on a serial port.
#[must_use]
pub fn with_vocoder(mut form: Vec<SettingField>) -> Vec<SettingField> {
    if chip::VOCODER_CHIPS {
        form.push(
            SettingField::new("vocoder", "Vocoder", Control::serial_port("blip25")).title(
                "blip25 in software, or a DVSI AMBE-3000/3003 chip such as a ThumbDV or USB-3003",
            ),
        );
    }
    form
}

/// The voice privacy keys a decoder undoes calls with, one per key ID the calls name.
#[must_use]
pub fn keys_field(title: &str, max_key_id: u16, digits: &str) -> SettingField {
    let entry = vec![
        SettingField::new(
            "key_id",
            "Key ID",
            NumberSpec::new(0.0, f64::from(max_key_id), 1.0),
        ),
        SettingField::new(
            "key",
            "Key",
            Control::bounded_text(digits, MAX_KEY_DIGITS, true),
        ),
    ];
    SettingField::new("keys", "Keys", Control::list(entry, "none"))
        .title(title)
        .quiet(Vec::<VoiceKey>::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_port_means_software() {
        assert!(matches!(open_chip(None, ChipRate::Half), Ok(None)));
        assert!(matches!(open_chip(Some(""), ChipRate::Half), Ok(None)));
    }

    #[test]
    fn a_chip_is_offered_only_in_builds_that_drive_one() {
        let form = with_vocoder(vec![keys_field("Keys", 255, "hex")]);
        let offered = form.iter().any(|field| field.key == "vocoder");
        assert_eq!(offered, chip::VOCODER_CHIPS);
    }

    #[test]
    fn speech_marks_the_audio_rate() {
        let mut out = ChannelOutputs::default();
        Speaker(&mut out).speak(&[0.25; 4]);
        assert_eq!(out.audio_pcm, [0.25; 4]);
        assert_eq!(out.audio_rate, AUDIO_RATE);
    }
}
