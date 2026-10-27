#![no_main]

use libfuzzer_sys::fuzz_target;

const LAYER_TWO: u8 = 8;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, bytes)) = data.split_first() else {
        return;
    };
    match selector % (LAYER_TWO + 1) {
        LAYER_TWO => sdrmm_channels::decode_layer_two(bytes),
        kind => sdrmm_channels::decode_bitstream(kind, bytes),
    }
});
