//! The guest side of the array processor ABI, instantiated for one processor type by
//! [`crate::export_processor!`].

use sdrmm_array::{ArrayProcessor, Settings};
use sdrmm_wire::plugin::processor::{ArrayFacts, ArrayHeader, ArrayOpen, LANE_OUTPUTS, Reset};

use super::{answer, fail, store, take, with_stored};
use crate::{
    Parts,
    output::{address, len_u32},
    processor::Hosted,
};

struct ArraySlot {
    hosted: Hosted,
    header: [u8; ArrayHeader::BYTES],
}

fn missing(handle: i32) -> String {
    format!("no processor {handle}")
}

fn opened<P, S>(parts: &Parts, bytes: &[u8]) -> Result<Hosted, String>
where
    P: ArrayProcessor + 'static,
    S: Settings,
{
    let kind = parts
        .processor()
        .ok_or_else(|| "this plugin declares no processor".to_owned())?;
    let request: ArrayOpen = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    Hosted::open::<P, S>(kind, request)
}

pub fn open<P, S>(parts: &Parts, ptr: u32, len: u32) -> i32
where
    P: ArrayProcessor + 'static,
    S: Settings,
{
    match opened::<P, S>(parts, &take(ptr, len)) {
        Ok(hosted) => store(ArraySlot {
            hosted,
            header: [0; ArrayHeader::BYTES],
        }),
        Err(error) => fail(&error),
    }
}

fn with_slot<R>(handle: i32, f: impl FnOnce(&mut ArraySlot) -> R) -> Option<R> {
    with_stored::<ArraySlot, _>(handle, f)
}

pub fn apply(handle: i32, ptr: u32, len: u32) -> i32 {
    let settings = crate::settings::<serde_json::Value>(&take(ptr, len));
    answer(settings.and_then(|settings| {
        with_slot(handle, |slot| slot.hosted.apply(settings))
            .unwrap_or_else(|| Err(missing(handle)))
    }))
}

pub fn retune(handle: i32, ptr: u32, len: u32) -> i32 {
    let facts =
        serde_json::from_slice::<ArrayFacts>(&take(ptr, len)).map_err(|error| error.to_string());
    answer(facts.and_then(|facts| {
        with_slot(handle, |slot| slot.hosted.retune(facts)).unwrap_or_else(|| Err(missing(handle)))
    }))
}

pub fn reset(handle: i32, cause: u32) {
    if let Some(cause) = Reset::from_code(cause) {
        with_slot(handle, |slot| slot.hosted.reset(cause));
    }
}

pub fn action(handle: i32, code: u32) -> i32 {
    answer(
        with_slot(handle, |slot| slot.hosted.action(code)).unwrap_or_else(|| Err(missing(handle))),
    )
}

pub fn input(handle: i32, lanes: u32, samples: u32) -> u32 {
    with_slot(handle, |slot| {
        address(slot.hosted.input(lanes as usize, samples as usize))
    })
    .unwrap_or(0)
}

pub fn context(handle: i32) -> u32 {
    with_slot(handle, |slot| address(slot.hosted.context())).unwrap_or(0)
}

pub fn process(handle: i32, _lanes: u32, samples: u32) -> u32 {
    with_slot(handle, |slot| {
        let staged = slot.hosted.process(samples as usize);
        let mut header = staged.header;
        header.values_ptr = address(&staged.values);
        header.values_len = len_u32(staged.values.len());
        header.events_ptr = address(&staged.events);
        header.events_len = len_u32(staged.events.len());
        header.surface_ptr = address(&staged.surface);
        header.surface_len = len_u32(staged.surface.len());
        for port in 0..LANE_OUTPUTS {
            header.lane_ptr[port] = address(&staged.lanes[port]);
            header.lane_samples[port] = len_u32(staged.lanes[port].len() / 2);
        }
        slot.header = header.to_bytes();
        address(&slot.header)
    })
    .unwrap_or(0)
}
