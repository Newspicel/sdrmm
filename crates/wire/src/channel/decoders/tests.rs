use std::collections::BTreeSet;

use crate::channel::{CHANNEL_TYPE_IDS, ChannelParams, home_frequency_hz};

#[test]
fn every_decoder_is_named_once() {
    let unique: BTreeSet<&str> = CHANNEL_TYPE_IDS.iter().copied().collect();
    assert_eq!(unique.len(), CHANNEL_TYPE_IDS.len());
    assert!(CHANNEL_TYPE_IDS.len() > 60);
}

#[test]
fn a_table_row_answers_to_its_own_wire_name() {
    for &type_id in CHANNEL_TYPE_IDS {
        let params = ChannelParams::default_for(type_id)
            .unwrap_or_else(|| panic!("{type_id} has no default settings"));
        assert_eq!(params.type_id(), type_id);
    }
}

#[test]
fn a_home_frequency_is_a_frequency() {
    for &type_id in CHANNEL_TYPE_IDS {
        if let Some(hz) = home_frequency_hz(type_id) {
            assert!(hz > 0.0 && hz.is_finite(), "{type_id} is at {hz} Hz");
        }
    }
    assert!(home_frequency_hz("telefax").is_none());
}
