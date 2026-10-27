use crate::proto::{fields, snd};

pub fn read_frame(bytes: &[u8]) {
    for field in fields(bytes) {
        let _ = field;
    }
    let Some(found) = snd(bytes) else {
        return;
    };
    let _ = (found.is_iq(), found.little_endian());
    assert!(
        found.pairs() * 4 <= found.iq.len(),
        "a KiwiSDR frame claimed more sample pairs than it carries"
    );
}
