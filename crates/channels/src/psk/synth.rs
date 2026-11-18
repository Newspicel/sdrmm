use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, Render, Rendered, Signal, resample};
use sdrmm_modem::{
    constellation::tables,
    linear::{LinearMod, LinearParams},
    pulse::{self, Norm},
};

use sdrmm_modem::synth::{Need, SynthError};
use sdrmm_modem::varicode::VARICODE;

const RATE: f64 = 8_000.0;
const PREAMBLE_BITS: usize = 32;
const POSTAMBLE_BITS: usize = 16;

pub fn transmission(text: &str, baud: f64) -> Result<Vec<Complex<f32>>, SynthError> {
    let mut bits = vec![false; PREAMBLE_BITS];
    for byte in text.bytes() {
        bits.extend(VARICODE[usize::from(byte)].bytes().map(|bit| bit == b'1'));
        bits.extend([false, false]);
    }
    bits.extend(std::iter::repeat_n(true, POSTAMBLE_BITS));

    let mut state = 0u32;
    let symbols: Vec<u32> = std::iter::once(state)
        .chain(bits.into_iter().map(|bit| {
            if !bit {
                state ^= 1;
            }
            state
        }))
        .collect();
    let sps = (RATE / baud).round() as usize;
    let pulse = pulse::root_raised_cosine(sps as f64, 1.0, 4, Norm::Energy);
    let params =
        LinearParams::new(tables::psk(2).need("BPSK table")?, pulse, sps).need("PSK waveform")?;
    Ok(LinearMod::transmission(&params, &symbols))
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "psk31",
    label: "PSK31",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(psk31),
}];

fn psk31() -> Rendered {
    let iq = transmission("CQ CQ DE SDRmm \n", crate::psk::PskBaud::Psk31.rate())?;
    Ok(resample(&iq, 8_000.0, AUDIO_HZ))
}
