//! The guest side of the encoder ABI, instantiated by [`crate::export!`].

use num_complex::Complex;
use sdrmm_decoder::encoder::{Modulator, TxPayload};
use sdrmm_wire::plugin::{encoder::PayloadKind, pack};

use super::{answer, fail, store, take, with_stored};
use crate::{
    Parts,
    output::{address, len_u32},
};

struct TxSlot {
    tx: Box<dyn Modulator>,
    output: Vec<Complex<f32>>,
}

pub fn open(parts: &Parts, ptr: u32, len: u32) -> i32 {
    match parts.modulate(&take(ptr, len)) {
        Ok(tx) => store(TxSlot {
            tx,
            output: Vec::new(),
        }),
        Err(error) => fail(&error),
    }
}

fn payload(kind: PayloadKind, bytes: Vec<u8>) -> TxPayload {
    match kind {
        PayloadKind::Frame => TxPayload::Frame(bytes),
        PayloadKind::Audio => TxPayload::Audio(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect(),
        ),
    }
}

pub fn submit(handle: i32, kind: u32, ptr: u32, len: u32) -> i32 {
    let bytes = take(ptr, len);
    let Some(kind) = PayloadKind::from_code(kind) else {
        return fail(&format!("unknown payload kind {kind}"));
    };
    let payload = payload(kind, bytes);
    answer(
        with_stored::<TxSlot, _>(handle, |slot| {
            slot.tx.submit(payload).map_err(|error| error.to_string())
        })
        .unwrap_or_else(|| Err(format!("no encoder {handle}"))),
    )
}

pub fn generate(handle: i32, max: u32) -> i64 {
    with_stored::<TxSlot, _>(handle, |slot| {
        slot.output.resize(max as usize, Complex::new(0.0, 0.0));
        let written = slot.tx.generate(&mut slot.output).min(slot.output.len());
        pack(address(&slot.output), len_u32(written))
    })
    .unwrap_or(0)
}
