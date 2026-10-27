pub mod acars;
pub mod adsb;
pub mod ais;
pub mod apt;
pub mod atv;
pub mod ble;
pub mod catalog;
pub mod dab;
mod dab_packet;
mod dab_pad;
pub mod datv;
#[cfg(any(test, feature = "fixtures"))]
pub mod decoder_fixtures;
pub mod dect;
pub mod drm;
pub mod dv;
pub mod dvbt;
pub mod eot;
pub mod ermes;
pub mod flex;
pub mod gnss;
pub mod gsm;
#[cfg(any(test, feature = "fixtures"))]
pub mod ident_fixtures;
pub mod ism;
pub mod lora;
pub mod lrpt;
pub mod morse;
pub mod navtex;
pub mod nfm;
pub mod noise_figure;
pub mod per;
pub mod pocsag;
pub mod psk;
pub mod radio_clock;
pub mod radiosonde;
pub mod rds;
pub mod remote_id;
pub mod rtty;
pub mod same;
pub mod sarsat;
pub mod selcall;
pub mod sid;
pub mod sstv;
pub mod tetra;
pub mod tetrapol;
pub mod vor;
pub mod weak_signal;
pub mod wefax;
pub mod wfm;
pub mod wmbus;

use num_complex::Complex;

pub use sdrmm_modem::synth::{
    Signal, add_noise, fm_modulate, fsk, ook, resample, scale, shift, silence, tone_audio,
};

use crate::ChannelTx;

#[must_use]
pub fn burst(tx: &mut dyn ChannelTx) -> Vec<Complex<f32>> {
    let mut iq = Vec::new();
    let mut block = [Complex::new(0.0, 0.0); 4_096];
    loop {
        let n = tx.generate(&mut block);
        iq.extend_from_slice(&block[..n]);
        if n < block.len() {
            return iq;
        }
    }
}
