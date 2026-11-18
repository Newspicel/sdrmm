use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, NARROW_HZ, Render, Signal, resample, silence};

use super::{BssPacket, Compose};

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "bss",
    label: "BSS · Benshi chat",
    rate_hz: NARROW_HZ,
    render: Render::Always(bss),
}];

fn bss() -> Vec<Complex<f32>> {
    let berlin = BssPacket {
        lat: Some(52.5163),
        lon: Some(13.3777),
        altitude_m: Some(34),
        speed_kmh: Some(0),
        heading_deg: Some(90),
        ..BssPacket::default()
    };
    let potsdam = BssPacket {
        lat: Some(52.4009),
        lon: Some(13.0591),
        altitude_m: Some(41),
        speed_kmh: Some(12),
        heading_deg: Some(250),
        ..BssPacket::default()
    };
    let frames = [
        Compose::from("DL1ABC-7")
            .position(&berlin)
            .message("CQ from the Tor"),
        Compose::from("DO2XYZ")
            .position(&potsdam)
            .message("loud and clear, 73"),
        Compose::from("DL1ABC-7").call_request(2, "DO2XYZ"),
    ];
    let mut iq = Vec::new();
    for frame in &frames {
        iq.extend(sdrmm_modem::packet::afsk_burst(frame.bytes(), AUDIO_HZ));
        iq.extend(silence(AUDIO_HZ as usize));
    }
    resample(&iq, AUDIO_HZ, NARROW_HZ)
}
