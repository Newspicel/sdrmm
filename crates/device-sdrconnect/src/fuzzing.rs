use crate::proto::{Payload, decode};

pub fn read_message(bytes: &[u8]) {
    if let Some((payload, body)) = Payload::split(bytes) {
        assert!(
            body.len() < bytes.len(),
            "{payload:?} kept the whole message as its body"
        );
    }
    let Ok(text) = str::from_utf8(bytes) else {
        return;
    };
    let _ = decode(text);
}
