use std::sync::Arc;

use sdrmm_device::{DeviceError, LaneEvent, RxSink, Sample, SinkForward, SweepSink};

use crate::abi::{EventV1, Handle, HostV1, Str, encode_error};

struct Link {
    ctx: Handle,
    host: &'static HostV1,
    release: extern "C" fn(Handle),
}

/// The host's sink context may be called from any thread; one sink is fed by one thread.
unsafe impl Send for Link {}
/// Shared use only reaches `sink_fail`, which the host serializes.
unsafe impl Sync for Link {}

impl Drop for Link {
    fn drop(&mut self) {
        (self.release)(self.ctx);
    }
}

struct Forward(Arc<Link>);

impl SinkForward for Forward {
    fn samples(&mut self, samples: &[Sample], _index: u64) {
        (self.0.host.sink_samples)(self.0.ctx, samples.as_ptr(), samples.len());
    }

    fn event(&mut self, event: LaneEvent) {
        (self.0.host.sink_event)(self.0.ctx, EventV1::encode(event));
    }

    fn dropped(&mut self, samples: u64) {
        (self.0.host.sink_dropped)(self.0.ctx, samples);
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        if std::thread::panicking() {
            fail_rx(
                &self.0,
                &DeviceError::Io("plugin stream panicked".to_owned()),
            );
        }
    }
}

fn fail_rx(link: &Link, error: &DeviceError) {
    let (code, text) = encode_error(error);
    (link.host.sink_fail)(link.ctx, code, Str::of(text.as_bytes()));
}

/// A receive sink that hands everything to the host's sink `ctx`, releasing it when the last
/// copy of its failure handle goes.
pub(crate) fn rx_sink(host: &'static HostV1, ctx: Handle) -> RxSink {
    let link = Arc::new(Link {
        ctx,
        host,
        release: host.sink_release,
    });
    let fatal = link.clone();
    RxSink::forwarding(Forward(link), move |error| fail_rx(&fatal, &error))
}

/// A sweep sink feeding the host's sweep `ctx`.
pub(crate) fn sweep_sink(host: &'static HostV1, ctx: Handle) -> SweepSink {
    let link = Arc::new(Link {
        ctx,
        host,
        release: host.sweep_release,
    });
    let fatal = link.clone();
    SweepSink::with_fatal_handler(
        move |center_hz, samples: &[Sample]| {
            (link.host.sweep_push)(link.ctx, center_hz, samples.as_ptr(), samples.len());
        },
        move |error| {
            let (code, text) = encode_error(&error);
            (fatal.host.sweep_fail)(fatal.ctx, code, Str::of(text.as_bytes()));
        },
    )
}
