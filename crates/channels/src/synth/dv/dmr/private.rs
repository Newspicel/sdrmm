use num_complex::Complex;
use sdrmm_dsp::{Bptc128, crc16_msb, pack_msb};

use super::{
    Call, DT_PI_HEADER, DT_TERMINATOR_WITH_LC, DT_VOICE_LC_HEADER, Keyed, bits, data_burst,
    embedded_lc, parity_lc, voice_burst,
};
use crate::dv::dmr::privacy::{CipherKey, FRAME_BYTES, Keystream, SILENCE, basic_keystream};
use crate::synth::dv::half_rate_air;

const FRAMES_PER_SUPERFRAME: usize = 18;
const FRAME_AIR_BITS: usize = 72;
const MOTOROLA: u8 = 0x10;
const DMRA: u8 = 0x20;
const PI_HEADER_MASK: u16 = 0x6969;

/// How a synthesized call hides its voice.
pub enum Privacy {
    MotorolaBasic(u16),
    /// ARC4 or AES, as the key's length says.
    Keyed {
        key_id: u8,
        key: String,
        mi: u32,
    },
}

/// An encrypted voice call of whole superframes. `frames` are natural-order AMBE+2 frames, 18 per
/// superframe, enciphered the way the radios do it.
#[must_use]
pub fn private_transmission(
    call: &Call,
    privacy: &Privacy,
    frames: &[[u8; FRAME_BYTES]],
    rate: f64,
) -> Vec<Complex<f32>> {
    let call = Call {
        color_code: call.color_code,
        group: call.group,
        encrypted: true,
        destination: call.destination,
        source: call.source,
    };
    let lc = motorola_lc(&call);
    let mut tx = Keyed::default();
    tx.burst(&data_burst(
        &call,
        DT_VOICE_LC_HEADER,
        &parity_lc(lc.clone(), [0x96, 0x96, 0x96]),
    ));
    if let Privacy::Keyed { key_id, key, mi } = privacy {
        let algorithm = DMRA | cipher_key(key).algorithm();
        tx.burst(&data_burst(
            &call,
            DT_PI_HEADER,
            &pi_header(&call, algorithm, *key_id, *mi),
        ));
    }
    let embedded = Bptc128::encode(&embedded_lc(&lc));
    let enciphered = encipher(privacy, frames);
    for superframe in enciphered.as_chunks::<FRAMES_PER_SUPERFRAME>().0 {
        let voice = sockets(superframe);
        tx.burst(&voice_burst(&call, None, &voice[0]));
        for (i, lcss) in [0b01u8, 0b11, 0b11, 0b10].into_iter().enumerate() {
            let fragment = embedded[i * 32..(i + 1) * 32].to_vec();
            tx.burst(&voice_burst(&call, Some((lcss, fragment)), &voice[i + 1]));
        }
        tx.burst(&voice_burst(&call, Some((0, vec![false; 32])), &voice[5]));
    }
    tx.burst(&data_burst(
        &call,
        DT_TERMINATOR_WITH_LC,
        &parity_lc(lc, [0x99, 0x99, 0x99]),
    ));
    tx.modulate(rate)
}

fn motorola_lc(call: &Call) -> Vec<bool> {
    let mut lc = call.link_control();
    lc[8..16].copy_from_slice(&bits(u64::from(MOTOROLA), 8));
    lc
}

fn cipher_key(hex: &str) -> CipherKey {
    CipherKey::parse(hex).unwrap_or_else(|| panic!("{hex:?} is no ARC4 or AES key"))
}

fn pi_header(call: &Call, algorithm: u8, key_id: u8, mi: u32) -> Vec<bool> {
    let mut payload = bits(u64::from(algorithm), 8);
    payload.extend(bits(u64::from(MOTOROLA), 8));
    payload.extend(bits(u64::from(key_id), 8));
    payload.extend(bits(u64::from(mi), 32));
    payload.extend(bits(u64::from(call.destination), 24));
    let crc = !crc16_msb(0x1021, 0, &pack_msb(&payload)) ^ PI_HEADER_MASK;
    payload.extend(bits(u64::from(crc), 16));
    payload
}

fn encipher(privacy: &Privacy, frames: &[[u8; FRAME_BYTES]]) -> Vec<[u8; FRAME_BYTES]> {
    match privacy {
        Privacy::MotorolaBasic(key) => {
            let stream = basic_keystream(*key);
            frames
                .iter()
                .map(|frame| std::array::from_fn(|i| frame[i] ^ stream[i]))
                .collect()
        }
        Privacy::Keyed { key, mi, .. } => {
            let mut keystream = Keystream::new(cipher_key(key), *mi);
            let mut out = Vec::with_capacity(frames.len());
            for (index, frame) in frames.iter().enumerate() {
                let position = index % FRAMES_PER_SUPERFRAME;
                if index > 0 && position == 0 {
                    keystream.next_superframe();
                }
                let stream = keystream.frame(position);
                out.push(if *frame == SILENCE {
                    *frame
                } else {
                    std::array::from_fn(|i| frame[i] ^ stream[i])
                });
            }
            out
        }
    }
}

fn sockets(superframe: &[[u8; FRAME_BYTES]]) -> [[bool; 216]; 6] {
    let mut sockets = [[false; 216]; 6];
    for (index, frame) in superframe.iter().enumerate() {
        let at = index % 3 * FRAME_AIR_BITS;
        sockets[index / 3][at..at + FRAME_AIR_BITS].copy_from_slice(&half_rate_air(frame));
    }
    sockets
}
