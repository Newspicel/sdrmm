//! A template for plugins: a decoder ([`Carrier`]) that reports the strongest unmodulated
//! carrier in its passband, an encoder ([`Beacon`]) and an array processor ([`LaneMeter`]).
//!
//! The decoder and the encoder are written the way built-in ones are, with the same traits and
//! builders. Build it in this folder with `cargo build --target wasm32-unknown-unknown --release`
//! and drop the `.wasm` into the server's plugins directory.

mod beacon;
mod carrier;
pub mod lane_meter;

pub use beacon::{Beacon, BeaconSettings};
pub use carrier::{Carrier, CarrierSeen, CarrierSettings};
pub use lane_meter::LaneMeter;
use sdrmm_plugin::{Decoder, Encoder, Plugin, wire::ProcessorType};

/// The plugin as a whole: the parts it brings.
pub struct Example;

impl Plugin for Example {
    fn decoder() -> Option<Box<dyn Decoder>> {
        Some(Box::new(carrier::decoder()))
    }

    fn encoder() -> Option<Box<dyn Encoder>> {
        Some(Box::new(beacon::encoder()))
    }

    fn processor() -> Option<ProcessorType> {
        Some(lane_meter::kind())
    }
}

sdrmm_plugin::export!(Example);
sdrmm_plugin::export_processor!(Example, LaneMeter, lane_meter::LaneMeterSettings);

#[cfg(test)]
mod tests {
    use sdrmm_plugin::{
        Parts,
        wire::plugin::{ABI_VERSION, Op, PresentRequest, Reply},
    };

    use super::*;

    #[test]
    fn the_manifest_lists_every_part_and_the_decoders_kinds() {
        let manifest = Parts::of::<Example>().manifest();
        assert_eq!(manifest.abi, ABI_VERSION);
        let decoder = manifest.decoder.expect("a decoder");
        assert_eq!(decoder.descriptor.type_id, "carrier");
        assert_eq!(decoder.descriptor.settings.len(), 2);
        assert!(decoder.descriptor.defaults.is_some());
        assert_eq!(decoder.feeds.len(), 1);
        assert_eq!(manifest.encoder.expect("an encoder").type_id, "beacon");
        assert_eq!(
            manifest.processor.expect("a processor").type_id,
            "lane_meter"
        );
        assert_eq!(manifest.kinds.len(), 1);
    }

    #[test]
    fn an_event_presents_through_its_kind() {
        let parts = Parts::of::<Example>();
        let request = PresentRequest {
            kind: carrier::KIND,
            data: sdrmm_plugin::serde_json::json!({ "offset_hz": -250.0, "snr_db": 31.5 }),
        };
        let bytes = sdrmm_plugin::serde_json::to_vec(&request).expect("request");
        let view = parts.present(&bytes).expect("presents");
        assert_eq!(view.summary, "Carrier -250 Hz");
    }

    #[test]
    fn a_control_call_answers_with_a_reply() {
        let parts = Parts::of::<Example>();
        let reply: Reply<f64> =
            sdrmm_plugin::serde_json::from_slice(&parts.call(Op::Width as i32, br#"{"baud":50}"#))
                .expect("a reply");
        assert_eq!(
            reply.into_result(),
            Ok(beacon::width_hz(&BeaconSettings { baud: 50.0 }))
        );
        let unknown: Reply<()> =
            sdrmm_plugin::serde_json::from_slice(&parts.call(99, b"")).expect("a reply");
        assert!(unknown.into_result().is_err());
    }
}
