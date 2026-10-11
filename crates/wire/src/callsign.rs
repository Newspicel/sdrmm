use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::DecoderEvent;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Callsign {
    pub call: String,
    pub country: String,
    pub prefix: String,
    pub continent: String,
    pub cq_zone: u8,
    pub itu_zone: u8,
}

impl DecoderEvent {
    #[must_use]
    pub fn amateur_callsign(&self) -> Option<&str> {
        match self {
            Self::Aprs(packet) => Some(&packet.source),
            Self::Wspr(spot) => Some(&spot.callsign),
            Self::Dv(frame) => frame.source_call.as_deref(),
            Self::Ft8(message) | Self::Ft4(message) => wsjt_sender(&message.text),
            _ => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AprsPacket, WsjtMessage};

    fn ft8(text: &str) -> DecoderEvent {
        DecoderEvent::Ft8(WsjtMessage {
            text: text.to_owned(),
            snr_db: 0.0,
            audio_hz: 0.0,
            time_offset_s: 0.0,
            hard_errors: 0,
        })
    }

    #[test]
    fn a_wsjt_message_names_its_sender() {
        assert_eq!(ft8("CQ DL1ABC JO62").amateur_callsign(), Some("DL1ABC"));
        assert_eq!(ft8("CQ DX DL1ABC JO62").amateur_callsign(), Some("DL1ABC"));
        assert_eq!(ft8("CQ POTA W1AW FN31").amateur_callsign(), Some("W1AW"));
        assert_eq!(ft8("W1AW DL1ABC -12").amateur_callsign(), Some("DL1ABC"));
        assert_eq!(ft8("CQ").amateur_callsign(), None);
    }

    #[test]
    fn only_amateur_decoders_carry_a_callsign() {
        let aprs = DecoderEvent::Aprs(AprsPacket {
            source: "DL1ABC-9".to_owned(),
            ..AprsPacket::default()
        });
        assert_eq!(aprs.amateur_callsign(), Some("DL1ABC-9"));
        assert_eq!(
            DecoderEvent::Adsb(crate::AdsbMessage::default()).amateur_callsign(),
            None
        );
    }
}
