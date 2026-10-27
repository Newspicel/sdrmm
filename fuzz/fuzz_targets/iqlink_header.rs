#![no_main]

use libfuzzer_sys::fuzz_target;
use sdrmm_iqlink::{HEADER_BYTES, Header, Reply, Start, frames_per_datagram};

fuzz_target!(|data: &[u8]| {
    if let Ok(header) = Header::decode(data) {
        assert_eq!(
            Header::decode(&header.encode()),
            Ok(header),
            "a decoded iqlink header did not survive re-encoding"
        );
        let frames = frames_per_datagram(usize::from(header.frame_bytes));
        assert!(
            frames * usize::from(header.frame_bytes) <= sdrmm_iqlink::DATAGRAM_BYTES - HEADER_BYTES,
            "iqlink claimed more frames than a datagram holds"
        );
    }
    let Ok(line) = str::from_utf8(data) else {
        return;
    };
    if let Ok(start) = Start::parse(line) {
        assert_eq!(
            Start::parse(&start.line()),
            Ok(start),
            "an iqlink START line did not survive re-encoding"
        );
    }
    if let Ok(reply) = Reply::parse(line) {
        assert_eq!(
            Reply::parse(&reply.line()),
            Ok(reply),
            "an iqlink reply line did not survive re-encoding"
        );
    }
});
