use crate::proto::{GREETING_LEN, Greeting};

pub fn read_greeting(bytes: &[u8]) {
    let Some(head) = bytes.first_chunk::<GREETING_LEN>() else {
        return;
    };
    let _ = Greeting::parse(head);
}
