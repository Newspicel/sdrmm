use crate::per::PerParams;
use num_complex::Complex;
use sdrmm_modem::synth::{NARROW_HZ, Render, Signal};

use crate::per::{deviation_hz, frame::frame_bits};
use sdrmm_modem::synth::{fsk, silence};

const LEAD_S: f64 = 0.01;

#[must_use]
pub fn frames(
    params: &PerParams,
    sequences: impl IntoIterator<Item = u32>,
    rate: f64,
) -> Vec<Complex<f32>> {
    let bits: Vec<bool> = sequences
        .into_iter()
        .flat_map(|sequence| frame_bits(sequence, params.payload_bytes))
        .collect();
    let lead = (LEAD_S * rate) as usize;
    let mut iq = silence(lead);
    iq.extend(fsk(
        &bits,
        params.baud.rate(),
        deviation_hz(params.baud),
        rate,
    ));
    iq.extend(silence(lead));
    iq
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "per_frames",
    label: "PER test frames",
    rate_hz: NARROW_HZ,
    render: Render::Always(per_frames),
}];

fn per_frames() -> Vec<Complex<f32>> {
    frames(&PerParams::default(), 0..100, NARROW_HZ)
}
