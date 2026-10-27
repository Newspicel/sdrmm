#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, frame)) = data.split_first() else {
        return;
    };
    match selector % 4 {
        0 => sdrmm_device_rtltcp::fuzzing::read_greeting(frame),
        1 => sdrmm_device_spyserver::fuzzing::read_message(frame),
        2 => sdrmm_device_kiwisdr::fuzzing::read_frame(frame),
        _ => sdrmm_device_sdrconnect::fuzzing::read_message(frame),
    }
});
