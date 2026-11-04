//! The Beast binary feed: Mode S frames as readsb, dump1090 and flight trackers read them.

use std::sync::Arc;

use sdrmm_wire::{DecoderEvent, EventContract, FeedEncoder, FeedSpec, KindSet};

use super::AdsbMessage;

const DEFAULT_ADDRESS: &str = "127.0.0.1:30005";
const HEARTBEAT: [u8; 11] = [0x1a, b'1', 0, 0, 0, 0, 0, 0, 0, 0, 0];

struct Beast;

impl FeedEncoder for Beast {
    fn encode(&self, event: &DecoderEvent) -> Result<Vec<u8>, String> {
        let message = event
            .read::<AdsbMessage>()
            .ok_or_else(|| format!("Beast carries ADS-B, not {}", event.kind))?;
        encode(&message).map_err(str::to_owned)
    }
}

pub(crate) fn feeds() -> Vec<FeedSpec> {
    vec![FeedSpec {
        format: "beast".to_owned(),
        name: "Beast".to_owned(),
        default_address: DEFAULT_ADDRESS.to_owned(),
        kinds: KindSet::EMPTY.with(AdsbMessage::KIND),
        heartbeat: HEARTBEAT.to_vec(),
        encoder: Arc::new(Beast),
    }]
}

fn encode(message: &AdsbMessage) -> Result<Vec<u8>, &'static str> {
    let kind = match message.raw.len() {
        14 => b'2',
        28 => b'3',
        _ => return Err("Invalid Mode S frame length"),
    };
    let timestamp = message
        .timestamp_12mhz
        .ok_or("ADS-B frame has no sample timestamp")?;
    let mut frame = Vec::with_capacity(44);
    frame.extend_from_slice(&[0x1a, kind]);
    for byte in &timestamp.to_be_bytes()[2..] {
        escape(*byte, &mut frame);
    }
    escape(message.signal_level.unwrap_or(0), &mut frame);
    for pair in message.raw.as_bytes().as_chunks::<2>().0 {
        let high = char::from(pair[0])
            .to_digit(16)
            .ok_or("Invalid Mode S hex")?;
        let low = char::from(pair[1])
            .to_digit(16)
            .ok_or("Invalid Mode S hex")?;
        escape((high * 16 + low) as u8, &mut frame);
    }
    Ok(frame)
}

fn escape(byte: u8, output: &mut Vec<u8>) {
    output.push(byte);
    if byte == 0x1a {
        output.push(byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> AdsbMessage {
        AdsbMessage {
            raw: "8D40621D58C382D690C8AC2863A7".to_owned(),
            timestamp_12mhz: Some(0x0102_0304_0506),
            signal_level: Some(128),
            ..AdsbMessage::default()
        }
    }

    #[test]
    fn encodes_known_long_and_short_frames_and_escapes_every_payload_field() {
        assert_eq!(
            encode(&message()).expect("a long frame"),
            [
                0x1a, b'3', 1, 2, 3, 4, 5, 6, 128, 0x8d, 0x40, 0x62, 0x1d, 0x58, 0xc3, 0x82, 0xd6,
                0x90, 0xc8, 0xac, 0x28, 0x63, 0xa7
            ]
        );
        let short = AdsbMessage {
            raw: "1a000000000000".to_owned(),
            timestamp_12mhz: Some(0x1a),
            signal_level: Some(0x1a),
            ..message()
        };
        assert_eq!(
            encode(&short).expect("a short frame"),
            [
                0x1a, b'2', 0, 0, 0, 0, 0, 0x1a, 0x1a, 0x1a, 0x1a, 0x1a, 0x1a, 0, 0, 0, 0, 0, 0
            ]
        );
        for raw in [
            "",
            "8D40621D58C382D690C8AC2863AZ",
            "8D40621D58C382D690C8AC2863",
        ] {
            assert!(
                encode(&AdsbMessage {
                    raw: raw.to_owned(),
                    ..message()
                })
                .is_err()
            );
        }
        assert!(
            encode(&AdsbMessage {
                timestamp_12mhz: None,
                ..message()
            })
            .is_err()
        );
    }

    #[test]
    fn the_feed_frames_adsb_events_and_refuses_others() {
        let feed = &feeds()[0];
        assert!(feed.kinds.contains(&AdsbMessage::KIND));
        let frame = feed.encoder.encode(&DecoderEvent::of(&message()));
        assert_eq!(frame, encode(&message()).map_err(str::to_owned));
        let other = DecoderEvent::tagged(sdrmm_wire::EventKind::named("other"), &1);
        assert!(feed.encoder.encode(&other).is_err());
    }
}
