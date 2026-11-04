//! The guest side of the array processor ABI, instantiated for one processor type by
//! [`crate::export_processor!`].

use num_complex::Complex;
use sdrmm_wire::plugin::processor::{ArrayFacts, ArrayHeader, ArrayOpen, Reset, lock};

use super::{address, fail, len_u32, store, take, with_stored};
use crate::{ArrayOutputs, Lanes, Processor, from_value, settings};

struct ArraySlot<T> {
    processor: T,
    input: Vec<Complex<f32>>,
    lanes: usize,
    out: ArrayOutputs,
    events: Vec<u8>,
    header: [u8; ArrayHeader::BYTES],
}

fn missing(handle: i32) -> String {
    format!("no processor {handle}")
}

fn answer(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => fail(&error),
    }
}

fn opened<T: Processor>(bytes: &[u8]) -> Result<T, String> {
    let request: ArrayOpen = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    T::open(&request.facts, from_value(request.settings)?)
}

pub fn open<T: Processor + 'static>(ptr: u32, len: u32) -> i32 {
    match opened::<T>(&take(ptr, len)) {
        Ok(processor) => store(ArraySlot {
            processor,
            input: Vec::new(),
            lanes: 0,
            out: ArrayOutputs::default(),
            events: Vec::new(),
            header: [0; ArrayHeader::BYTES],
        }),
        Err(error) => fail(&error),
    }
}

pub fn apply<T: Processor + 'static>(handle: i32, ptr: u32, len: u32) -> i32 {
    answer(settings::<T::Settings>(&take(ptr, len)).and_then(|s| {
        with_stored::<ArraySlot<T>, _>(handle, |slot| slot.processor.apply(s))
            .unwrap_or_else(|| Err(missing(handle)))
    }))
}

pub fn retune<T: Processor + 'static>(handle: i32, ptr: u32, len: u32) -> i32 {
    let facts =
        serde_json::from_slice::<ArrayFacts>(&take(ptr, len)).map_err(|error| error.to_string());
    answer(facts.and_then(|facts| {
        with_stored::<ArraySlot<T>, _>(handle, |slot| slot.processor.retune(&facts))
            .unwrap_or_else(|| Err(missing(handle)))
    }))
}

pub fn reset<T: Processor + 'static>(handle: i32, cause: u32) {
    if let Some(cause) = Reset::from_code(cause) {
        with_stored::<ArraySlot<T>, _>(handle, |slot| slot.processor.reset(cause));
    }
}

pub fn input<T: Processor + 'static>(handle: i32, lanes: u32, samples: u32) -> u32 {
    with_stored::<ArraySlot<T>, _>(handle, |slot| {
        slot.lanes = lanes as usize;
        let total = (lanes as usize).saturating_mul(samples as usize);
        slot.input.resize(total, Complex::new(0.0, 0.0));
        address(&slot.input)
    })
    .unwrap_or(0)
}

pub fn process<T: Processor + 'static>(handle: i32, lanes: u32, samples: u32) -> u32 {
    with_stored::<ArraySlot<T>, _>(handle, |slot| {
        slot.out.clear();
        let count = (lanes as usize).min(slot.lanes);
        let total = count.saturating_mul(samples as usize).min(slot.input.len());
        slot.processor
            .process(Lanes::new(&slot.input[..total], count), &mut slot.out);
        encode(slot)
    })
    .unwrap_or(0)
}

const fn lock_code(lock: Option<bool>) -> u32 {
    match lock {
        Some(true) => lock::HELD,
        Some(false) => lock::LOST,
        None => lock::UNKNOWN,
    }
}

fn encode<T>(slot: &mut ArraySlot<T>) -> u32 {
    slot.events.clear();
    if !slot.out.events.is_empty()
        && serde_json::to_writer(&mut slot.events, &slot.out.events).is_err()
    {
        slot.events.clear();
        slot.out.truncated = slot.out.truncated.saturating_add(1);
    }
    let health = slot.out.health;
    slot.header = ArrayHeader {
        report: u32::from(slot.out.report),
        values_ptr: address(&slot.out.values),
        values_len: len_u32(slot.out.values.len()),
        lock: lock_code(health.lock),
        snr_db: health.snr_db.unwrap_or(f32::NAN),
        freq_error_hz: health.freq_error_hz.map_or(f32::NAN, |hz| hz as f32),
        errors_per_s: health.errors_per_s.unwrap_or(f32::NAN),
        events_ptr: address(&slot.events),
        events_len: len_u32(slot.events.len()),
        truncated: slot.out.truncated,
    }
    .to_bytes();
    address(&slot.header)
}
