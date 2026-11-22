use std::{
    cell::UnsafeCell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use sdrmm_device::{FatalHandle, LaneEvent, RxSink, Sample, SweepSink};

use crate::abi::{EventV1, Handle, Str, decode_error};

/// A host sink lent to a plugin. The plugin feeds `sink` from one thread at a time, and may
/// fail it from any other, which is why failure goes through the shared `fatal` handle.
struct Lent {
    sink: UnsafeCell<RxSink>,
    fatal: FatalHandle,
}

pub(crate) fn lend(mut sink: RxSink) -> Handle {
    let fatal = sink.share_failure();
    Box::into_raw(Box::new(Lent {
        sink: UnsafeCell::new(sink),
        fatal,
    }))
    .cast()
}

/// Runs `call` on the lent sink behind `ctx`, which the plugin feeds from one thread at a time
/// until it releases it. A panic stays on this side of the boundary and fails the stream.
fn with_sink(ctx: Handle, call: impl FnOnce(&mut RxSink)) {
    let lent = unsafe { &*ctx.cast::<Lent>() };
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| call(unsafe { &mut *lent.sink.get() })))
    {
        let error = sdrmm_device::DeviceError::Io(format!(
            "the host sink panicked: {}",
            crate::guest::panic_text(&*payload)
        ));
        let _ = catch_unwind(AssertUnwindSafe(|| lent.fatal.fail(error)));
    }
}

pub(super) extern "C" fn samples(ctx: Handle, samples: *const Sample, len: usize) {
    with_sink(ctx, |sink| {
        let samples = if samples.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(samples, len) }
        };
        sink.push(samples);
    });
}

pub(super) extern "C" fn event(ctx: Handle, event: EventV1) {
    with_sink(ctx, |sink| match event.decode(sink.index()) {
        Some(LaneEvent::Mark { mark, .. }) => sink.mark(mark),
        Some(LaneEvent::Uncertain {
            error,
            scope,
            cause,
            ..
        }) => sink.realigned(cause, error, scope),
        Some(LaneEvent::HardwareTime { ns, .. }) => sink.stamp_hardware(ns),
        Some(LaneEvent::Deaf { samples, .. }) => sink.deaf(samples),
        None => tracing::warn!(
            kind = event.kind,
            "a device plugin sent a lane event this host does not know"
        ),
    });
}

pub(super) extern "C" fn dropped(ctx: Handle, samples: u64) {
    with_sink(ctx, |sink| sink.dropped(samples));
}

pub(super) extern "C" fn fail(ctx: Handle, code: u32, message: Str) {
    let lent = unsafe { &*ctx.cast::<Lent>() };
    let error = decode_error(code, unsafe { message.text() }.into_owned());
    let _ = catch_unwind(AssertUnwindSafe(|| lent.fatal.fail(error)));
}

pub(super) extern "C" fn release(ctx: Handle) {
    let _ = catch_unwind(|| drop(unsafe { Box::from_raw(ctx.cast::<Lent>()) }));
}

struct LentSweep(UnsafeCell<SweepSink>);

pub(crate) fn lend_sweep(sink: SweepSink) -> Handle {
    Box::into_raw(Box::new(LentSweep(UnsafeCell::new(sink)))).cast()
}

/// As [`with_sink`]: the plugin feeds and fails one sweep sink from one thread at a time.
fn with_sweep(ctx: Handle, call: impl FnOnce(&mut SweepSink)) {
    let lent = unsafe { &*ctx.cast::<LentSweep>() };
    let _ = catch_unwind(AssertUnwindSafe(|| call(unsafe { &mut *lent.0.get() })));
}

pub(super) extern "C" fn sweep_push(
    ctx: Handle,
    center_hz: f64,
    samples: *const Sample,
    len: usize,
) {
    with_sweep(ctx, |sink| {
        let samples = if samples.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(samples, len) }
        };
        sink.push(center_hz, samples);
    });
}

pub(super) extern "C" fn sweep_fail(ctx: Handle, code: u32, message: Str) {
    let error = decode_error(code, unsafe { message.text() }.into_owned());
    with_sweep(ctx, |sink| sink.fail(error));
}

pub(super) extern "C" fn sweep_release(ctx: Handle) {
    let _ = catch_unwind(|| drop(unsafe { Box::from_raw(ctx.cast::<LentSweep>()) }));
}
