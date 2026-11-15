//! The guest side of the encoder ABI, instantiated for one encoder type by
//! [`crate::export_encoder!`].

use num_complex::Complex;
use sdrmm_wire::plugin::{encoder::PayloadKind, pack};

use super::{address, fail, len_u32, store, take, with_stored};
use crate::{Encoder, Payload, settings};

struct TxSlot<T> {
    tx: T,
    audio: Vec<f32>,
    output: Vec<Complex<f32>>,
}

fn missing(handle: i32) -> String {
    format!("no encoder {handle}")
}

fn answer(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => fail(&error),
    }
}

pub fn open<T: Encoder + 'static>(rate: f64, ptr: u32, len: u32) -> i32 {
    let opened = settings::<T::Settings>(&take(ptr, len)).and_then(|s| T::open(rate, s));
    match opened {
        Ok(tx) => store(TxSlot {
            tx,
            audio: Vec::new(),
            output: Vec::new(),
        }),
        Err(error) => fail(&error),
    }
}

pub fn submit<T: Encoder + 'static>(handle: i32, kind: u32, ptr: u32, len: u32) -> i32 {
    let bytes = take(ptr, len);
    let Some(kind) = PayloadKind::from_code(kind) else {
        return fail(&format!("unknown payload kind {kind}"));
    };
    answer(
        with_stored::<TxSlot<T>, _>(handle, |slot| match kind {
            PayloadKind::Frame => slot.tx.submit(Payload::Frame(&bytes)),
            PayloadKind::Audio => {
                slot.audio.clear();
                slot.audio.extend(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|chunk| f32::from_le_bytes(*chunk)),
                );
                slot.tx.submit(Payload::Audio(&slot.audio))
            }
        })
        .unwrap_or_else(|| Err(missing(handle))),
    )
}

pub fn generate<T: Encoder + 'static>(handle: i32, max: u32) -> i64 {
    with_stored::<TxSlot<T>, _>(handle, |slot| {
        slot.output.resize(max as usize, Complex::new(0.0, 0.0));
        let written = slot.tx.generate(&mut slot.output).min(slot.output.len());
        pack(address(&slot.output), len_u32(written))
    })
    .unwrap_or(0)
}
