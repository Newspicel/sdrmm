//! The guest side of the plugin ABI, instantiated for one plugin type by [`crate::export!`].
//!
//! Every buffer the host reads stays owned by a `Vec` here until the next call, so the host only
//! ever reads memory the guest still holds.

use std::{any::Any, cell::RefCell, collections::HashMap};

use num_complex::Complex;
use sdrmm_wire::plugin::{Op, OutputHeader, PresentRequest, Reply, pack};
use serde::Serialize;

use crate::{Outputs, Plugin, settings};

struct Slot<P> {
    plugin: P,
    input: Vec<Complex<f32>>,
    out: Outputs,
    events: Vec<u8>,
    status: Vec<u8>,
    header: [u8; OutputHeader::BYTES],
}

thread_local! {
    static BUFFERS: RefCell<HashMap<u32, Vec<u8>>> = RefCell::new(HashMap::new());
    static REPLY: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static CHANNELS: RefCell<Vec<Option<Box<dyn Any>>>> = const { RefCell::new(Vec::new()) };
}

fn address<T>(slice: &[T]) -> u32 {
    slice.as_ptr() as usize as u32
}

fn len_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

pub fn alloc(len: u32) -> u32 {
    if len == 0 {
        return 0;
    }
    let buffer = vec![0u8; len as usize];
    let ptr = address(&buffer);
    BUFFERS.with_borrow_mut(|buffers| buffers.insert(ptr, buffer));
    ptr
}

fn take(ptr: u32, len: u32) -> Vec<u8> {
    let mut buffer = BUFFERS
        .with_borrow_mut(|buffers| buffers.remove(&ptr))
        .unwrap_or_default();
    buffer.truncate(len as usize);
    buffer
}

fn reply(bytes: Vec<u8>) -> i64 {
    let packed = pack(address(&bytes), len_u32(bytes.len()));
    REPLY.set(bytes);
    packed
}

fn json(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|error| {
        serde_json::to_vec(&Reply::<()>::Err(error.to_string())).unwrap_or_default()
    })
}

fn fail(error: &str) -> i32 {
    ERROR.set(error.as_bytes().to_vec());
    -1
}

pub fn error() -> i64 {
    ERROR.with_borrow(|error| pack(address(error), len_u32(error.len())))
}

pub fn manifest<P: Plugin>() -> i64 {
    reply(json(&P::manifest()))
}

pub fn call<P: Plugin>(op: i32, ptr: u32, len: u32) -> i64 {
    let bytes = take(ptr, len);
    let answer = match Op::from_code(op) {
        Some(Op::Layout) => json(&Reply::from(
            settings::<P::Settings>(&bytes).map(|s| P::layout(&s)),
        )),
        Some(Op::Normalize) => json(&Reply::from(
            settings::<P::Settings>(&bytes)
                .and_then(|s| serde_json::to_value(s).map_err(|error| error.to_string())),
        )),
        Some(Op::Present) => json(&Reply::from(
            serde_json::from_slice::<PresentRequest>(&bytes)
                .map_err(|error| error.to_string())
                .and_then(|request| P::present(&request.kind, &request.data)),
        )),
        None => json(&Reply::<()>::Err(format!("unknown op {op}"))),
    };
    reply(answer)
}

pub fn open<P: Plugin + 'static>(rate: f64, ptr: u32, len: u32) -> i32 {
    let opened = settings::<P::Settings>(&take(ptr, len)).and_then(|s| P::open(rate, s));
    match opened {
        Ok(plugin) => store(Slot {
            plugin,
            input: Vec::new(),
            out: Outputs::default(),
            events: Vec::new(),
            status: Vec::new(),
            header: [0; OutputHeader::BYTES],
        }),
        Err(error) => fail(&error),
    }
}

fn store<P: 'static>(slot: Slot<P>) -> i32 {
    CHANNELS.with_borrow_mut(|channels| {
        let boxed: Box<dyn Any> = Box::new(slot);
        let index = match channels.iter().position(Option::is_none) {
            Some(free) => {
                channels[free] = Some(boxed);
                free
            }
            None => {
                channels.push(Some(boxed));
                channels.len() - 1
            }
        };
        i32::try_from(index).unwrap_or(-1)
    })
}

fn with_slot<P: 'static, R>(handle: i32, f: impl FnOnce(&mut Slot<P>) -> R) -> Option<R> {
    let index = usize::try_from(handle).ok()?;
    CHANNELS.with_borrow_mut(|channels| {
        let slot = channels
            .get_mut(index)?
            .as_mut()?
            .downcast_mut::<Slot<P>>()?;
        Some(f(slot))
    })
}

pub fn apply<P: Plugin + 'static>(handle: i32, ptr: u32, len: u32) -> i32 {
    let applied = settings::<P::Settings>(&take(ptr, len)).and_then(|s| {
        with_slot::<P, _>(handle, |slot| slot.plugin.apply(s))
            .unwrap_or(Err(format!("no channel {handle}")))
    });
    match applied {
        Ok(()) => 0,
        Err(error) => fail(&error),
    }
}

pub fn input<P: Plugin + 'static>(handle: i32, samples: u32) -> u32 {
    with_slot::<P, _>(handle, |slot| {
        slot.input.resize(samples as usize, Complex::new(0.0, 0.0));
        address(&slot.input)
    })
    .unwrap_or(0)
}

pub fn process<P: Plugin + 'static>(handle: i32, samples: u32) -> u32 {
    with_slot::<P, _>(handle, |slot| {
        slot.out.clear();
        let n = (samples as usize).min(slot.input.len());
        slot.plugin.process(&slot.input[..n], &mut slot.out);
        encode(slot)
    })
    .unwrap_or(0)
}

fn encode<P>(slot: &mut Slot<P>) -> u32 {
    slot.events.clear();
    if !slot.out.events.is_empty()
        && serde_json::to_writer(&mut slot.events, &slot.out.events).is_err()
    {
        slot.events.clear();
        slot.out.skipped = slot.out.skipped.saturating_add(1);
    }
    slot.status.clear();
    if let Some(status) = &slot.out.status
        && serde_json::to_writer(&mut slot.status, status).is_err()
    {
        slot.status.clear();
    }
    slot.header = OutputHeader {
        audio_ptr: address(&slot.out.audio),
        audio_len: len_u32(slot.out.audio.len()),
        audio_rate: slot.out.audio_rate,
        events_ptr: address(&slot.events),
        events_len: len_u32(slot.events.len()),
        status_ptr: address(&slot.status),
        status_len: len_u32(slot.status.len()),
        rejected: slot.out.rejected,
        skipped: slot.out.skipped,
    }
    .to_bytes();
    address(&slot.header)
}

pub fn retuned<P: Plugin + 'static>(handle: i32) {
    with_slot::<P, _>(handle, |slot| slot.plugin.retuned());
}

pub fn close<P: 'static>(handle: i32) {
    if let Ok(index) = usize::try_from(handle) {
        CHANNELS.with_borrow_mut(|channels| {
            if let Some(slot) = channels.get_mut(index) {
                *slot = None;
            }
        });
    }
}
