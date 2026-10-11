use num_complex::Complex;
use sdrmm_wire::PerParams;

use super::{fsk, silence};
use crate::per::{deviation_hz, frame::frame_bits};

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
