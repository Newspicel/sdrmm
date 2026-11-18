//! The guest side of the plugin ABI, instantiated by [`crate::export!`]: it moves bytes between
//! the host and guest memory and hands each call to [`Parts`].
//!
//! Every buffer the host reads stays owned by a `Vec` here until the next call, so the host only
//! ever reads memory the guest still holds.

use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, UNIX_EPOCH},
};

use num_complex::Complex;
use sdrmm_decoder::{Channel, ChannelOutputs};
use sdrmm_wire::{
    PositionFix,
    plugin::{OutputHeader, Reply, pack, process::WANT_SYMBOLS},
};

use crate::{
    Parts, Plugin,
    guest::{json, parse},
    output::{OutputWire, address, len_u32},
};

pub mod encoder;
pub mod processor;

struct ChannelSlot {
    channel: Box<dyn Channel>,
    input: Vec<Complex<f32>>,
    out: ChannelOutputs,
    wire: OutputWire,
    header: [u8; OutputHeader::BYTES],
}

static PARTS: OnceLock<Parts> = OnceLock::new();

thread_local! {
    static BUFFERS: RefCell<HashMap<u32, Vec<u8>>> = RefCell::new(HashMap::new());
    static REPLY: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static SLOTS: RefCell<Vec<Option<Box<dyn Any>>>> = const { RefCell::new(Vec::new()) };
}

/// What plugin `P` brings, built on first use.
pub fn parts<P: Plugin>() -> &'static Parts {
    PARTS.get_or_init(Parts::of::<P>)
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

pub(crate) fn take(ptr: u32, len: u32) -> Vec<u8> {
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

pub(crate) fn fail(error: &str) -> i32 {
    ERROR.set(error.as_bytes().to_vec());
    -1
}

pub(crate) fn answer(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => fail(&error),
    }
}

pub fn error() -> i64 {
    ERROR.with_borrow(|error| pack(address(error), len_u32(error.len())))
}

pub fn manifest(parts: &Parts) -> i64 {
    reply(json(&parts.manifest()))
}

pub fn present(parts: &Parts, ptr: u32, len: u32) -> i64 {
    reply(json(&Reply::from(parts.present(&take(ptr, len)))))
}

pub fn call(parts: &Parts, op: i32, ptr: u32, len: u32) -> i64 {
    reply(parts.call(op, &take(ptr, len)))
}

pub(crate) fn store<T: 'static>(slot: T) -> i32 {
    SLOTS.with_borrow_mut(|slots| {
        let boxed: Box<dyn Any> = Box::new(slot);
        let index = if let Some(free) = slots.iter().position(Option::is_none) {
            slots[free] = Some(boxed);
            free
        } else {
            slots.push(Some(boxed));
            slots.len() - 1
        };
        i32::try_from(index).unwrap_or(-1)
    })
}

pub(crate) fn with_stored<T: 'static, R>(handle: i32, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    let index = usize::try_from(handle).ok()?;
    SLOTS.with_borrow_mut(|slots| {
        let slot = slots.get_mut(index)?.as_mut()?.downcast_mut::<T>()?;
        Some(f(slot))
    })
}

fn with_channel<R>(handle: i32, f: impl FnOnce(&mut ChannelSlot) -> R) -> Option<R> {
    with_stored::<ChannelSlot, R>(handle, f)
}

pub fn open(parts: &Parts, rate: f64, ptr: u32, len: u32) -> i32 {
    match parts.open(rate, &take(ptr, len)) {
        Ok(channel) => store(ChannelSlot {
            channel,
            input: Vec::new(),
            out: ChannelOutputs::default(),
            wire: OutputWire::default(),
            header: [0; OutputHeader::BYTES],
        }),
        Err(error) => fail(&error),
    }
}

pub fn apply(handle: i32, ptr: u32, len: u32) -> i32 {
    let applied = parse(&take(ptr, len)).and_then(|settings| {
        with_channel(handle, |slot| {
            slot.channel
                .apply(settings)
                .map_err(|error| error.to_string())
        })
        .unwrap_or_else(|| Err(format!("no channel {handle}")))
    });
    answer(applied)
}

pub fn input(handle: i32, samples: u32) -> u32 {
    with_channel(handle, |slot| {
        slot.input.resize(samples as usize, Complex::new(0.0, 0.0));
        address(&slot.input)
    })
    .unwrap_or(0)
}

pub fn process(handle: i32, samples: u32, flags: u32) -> u32 {
    with_channel(handle, |slot| {
        slot.out.reset();
        slot.out.symbols.set_wanted(flags & WANT_SYMBOLS != 0);
        let n = (samples as usize).min(slot.input.len());
        slot.channel.process(&slot.input[..n], &mut slot.out);
        let header = slot
            .wire
            .encode(&slot.out, slot.channel.rejected(), slot.channel.skipped());
        slot.header = header.to_bytes();
        address(&slot.header)
    })
    .unwrap_or(0)
}

pub fn retuned(handle: i32) {
    with_channel(handle, |slot| slot.channel.retuned());
}

pub fn position(handle: i32, ptr: u32, len: u32) {
    let Ok(fix) = parse::<Option<PositionFix>>(&take(ptr, len)) else {
        return;
    };
    with_channel(handle, |slot| slot.channel.position_changed(fix.as_ref()));
}

pub fn clock(handle: i32, unix_ns: i64) {
    let since = Duration::from_nanos(unix_ns.unsigned_abs());
    let time = if unix_ns >= 0 {
        UNIX_EPOCH.checked_add(since)
    } else {
        UNIX_EPOCH.checked_sub(since)
    };
    if let Some(time) = time {
        with_channel(handle, |slot| slot.channel.captured_at(time));
    }
}

pub fn lo_artifact(handle: i32, offset_hz: f64) {
    let at = (!offset_hz.is_nan()).then_some(offset_hz);
    with_channel(handle, |slot| slot.channel.lo_artifact_at(at));
}

pub fn gated(handle: i32) -> i32 {
    with_channel(handle, |slot| i32::from(slot.channel.needs_gated_input())).unwrap_or(1)
}

pub fn close(handle: i32) {
    if let Ok(index) = usize::try_from(handle) {
        SLOTS.with_borrow_mut(|slots| {
            if let Some(slot) = slots.get_mut(index) {
                *slot = None;
            }
        });
    }
}
