use std::time::Duration;

use sdrmm_device::{DeviceError, Sample, SdrDevice, SweepBand, SweepPlan, TxStream};
use sdrmm_wire::DeviceInfo;

use super::{GuestDriver, free_bytes, guard, put_json, sink, take_json};
use crate::abi::{
    Bytes, DEVICE_ABI_VERSION, Handle, HostV1, MAX_TX_CHANNELS, PluginV1, SamplesV1, Status, Str,
    SweepPlanV1,
};

struct GuestDevice {
    device: Box<dyn SdrDevice>,
    host: &'static HostV1,
}

/// Every pointer the host hands back is one this library gave it, alive until its drop call;
/// the host never calls one handle from two threads at once.
fn driver<'a>(handle: Handle) -> &'a GuestDriver {
    unsafe { &*handle.cast::<GuestDriver>() }
}

fn device<'a>(handle: Handle) -> &'a mut GuestDevice {
    unsafe { &mut *handle.cast::<GuestDevice>() }
}

fn tx<'a>(handle: Handle) -> &'a mut Box<dyn TxStream> {
    unsafe { &mut *handle.cast::<Box<dyn TxStream>>() }
}

pub(super) fn table(plugin: &'static str, driver: Handle) -> PluginV1 {
    PluginV1 {
        abi: DEVICE_ABI_VERSION,
        size: size_of::<PluginV1>() as u32,
        plugin: Str::of(plugin.as_bytes()),
        sdk_build: Str::of(env!("SDRMM_SDK_BUILD").as_bytes()),
        panic_unwind: cfg!(panic = "unwind"),
        driver,
        driver_drop,
        free_bytes,
        spec,
        probe,
        open,
        resolve,
        health,
        write_serial,
        capabilities,
        settings,
        apply,
        rx_start,
        rx_stop,
        duplex,
        in_flight_samples,
        tx_start,
        agc_gains,
        readouts,
        set_noise_source,
        sweep_start,
        sweep_stop,
        device_drop,
        tx_write,
        tx_stop,
        tx_drop,
    }
}

extern "C" fn driver_drop(handle: Handle) {
    let _ = guard(|| {
        drop(unsafe { Box::from_raw(handle.cast::<GuestDriver>()) });
        Ok(())
    });
}

extern "C" fn spec(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, &driver(handle).driver.spec()))
}

extern "C" fn probe(handle: Handle, deep: bool, out: *mut Bytes) -> Status {
    guard(|| {
        let driver = &driver(handle).driver;
        let found = if deep {
            driver.probe_deep()
        } else {
            driver.probe()
        };
        put_json(out, &found)
    })
}

extern "C" fn open(handle: Handle, info: Str, out: *mut Handle) -> Status {
    guard(|| {
        let guest = driver(handle);
        let info: DeviceInfo = take_json(info)?;
        let device = guest.driver.open(&info)?;
        let device = Box::new(GuestDevice {
            device,
            host: guest.host,
        });
        unsafe { out.write(Box::into_raw(device).cast()) };
        Ok(())
    })
}

extern "C" fn resolve(handle: Handle, key: Str, out: *mut Bytes) -> Status {
    guard(|| put_json(out, &driver(handle).driver.resolve(&unsafe { key.text() })))
}

extern "C" fn health(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, &driver(handle).driver.health()))
}

extern "C" fn write_serial(handle: Handle, key: Str, serial: Str, out: *mut Bytes) -> Status {
    guard(|| {
        let serial: Option<String> = take_json(serial)?;
        let written = driver(handle)
            .driver
            .write_serial(&unsafe { key.text() }, serial.as_deref())?;
        put_json(out, &written)
    })
}

extern "C" fn capabilities(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, device(handle).device.capabilities()))
}

extern "C" fn settings(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, device(handle).device.settings()))
}

extern "C" fn apply(handle: Handle, settings: Str) -> Status {
    guard(|| device(handle).device.apply(&take_json(settings)?))
}

extern "C" fn rx_start(handle: Handle, sinks: *const Handle, count: usize) -> Status {
    guard(|| {
        let guest = device(handle);
        let contexts = unsafe { std::slice::from_raw_parts(sinks, count) };
        let sinks = contexts
            .iter()
            .map(|&ctx| sink::rx_sink(guest.host, ctx))
            .collect();
        guest.device.rx_start(sinks)
    })
}

extern "C" fn rx_stop(handle: Handle) -> Status {
    guard(|| {
        device(handle).device.rx_stop();
        Ok(())
    })
}

extern "C" fn duplex(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, &device(handle).device.duplex()))
}

extern "C" fn in_flight_samples(handle: Handle, out: *mut u64) -> Status {
    guard(|| {
        let samples = device(handle).device.in_flight_samples();
        unsafe { out.write(samples) };
        Ok(())
    })
}

extern "C" fn tx_start(
    handle: Handle,
    channels: *const u32,
    count: usize,
    out: *mut Handle,
) -> Status {
    guard(|| {
        let channels = unsafe { std::slice::from_raw_parts(channels, count) };
        let stream = device(handle).device.tx_start_channels(channels)?;
        unsafe { out.write(Box::into_raw(Box::new(stream)).cast()) };
        Ok(())
    })
}

extern "C" fn agc_gains(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, &device(handle).device.agc_gains()?))
}

extern "C" fn readouts(handle: Handle, out: *mut Bytes) -> Status {
    guard(|| put_json(out, &device(handle).device.readouts()?))
}

extern "C" fn set_noise_source(handle: Handle, on: bool) -> Status {
    guard(|| device(handle).device.set_noise_source(on))
}

extern "C" fn sweep_start(handle: Handle, plan: *const SweepPlanV1, ctx: Handle) -> Status {
    let guest = device(handle);
    let sink = sink::sweep_sink(guest.host, ctx);
    guard(|| {
        let plan = unsafe { &*plan };
        let bands = unsafe { std::slice::from_raw_parts(plan.bands, plan.band_count) }
            .iter()
            .map(|&[start_hz, stop_hz]| SweepBand { start_hz, stop_hz })
            .collect();
        guest
            .device
            .sweep_start(&SweepPlan::new(bands, plan.sample_rate_hz), sink)
    })
}

extern "C" fn sweep_stop(handle: Handle) -> Status {
    guard(|| {
        device(handle).device.sweep_stop();
        Ok(())
    })
}

extern "C" fn device_drop(handle: Handle) {
    let _ = guard(|| {
        drop(unsafe { Box::from_raw(handle.cast::<GuestDevice>()) });
        Ok(())
    });
}

extern "C" fn tx_write(
    handle: Handle,
    channels: *const SamplesV1,
    count: usize,
    timeout_ns: u64,
    end_burst: bool,
    out: *mut usize,
) -> Status {
    guard(|| {
        if count > MAX_TX_CHANNELS {
            return Err(DeviceError::Unsupported(format!(
                "a plugin writes at most {MAX_TX_CHANNELS} channels at once, got {count}"
            )));
        }
        let given = unsafe { std::slice::from_raw_parts(channels, count) };
        let mut slices: [&[Sample]; MAX_TX_CHANNELS] = [&[]; MAX_TX_CHANNELS];
        for (slot, channel) in slices.iter_mut().zip(given) {
            *slot = unsafe { std::slice::from_raw_parts(channel.ptr, channel.len) };
        }
        let written = tx(handle).write_channels(
            &slices[..count],
            Duration::from_nanos(timeout_ns),
            end_burst,
        )?;
        unsafe { out.write(written) };
        Ok(())
    })
}

extern "C" fn tx_stop(handle: Handle) -> Status {
    guard(|| tx(handle).stop())
}

extern "C" fn tx_drop(handle: Handle) {
    let _ = guard(|| {
        drop(unsafe { Box::from_raw(handle.cast::<Box<dyn TxStream>>()) });
        Ok(())
    });
}
