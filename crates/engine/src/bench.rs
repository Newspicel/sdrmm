//! Runs one radio, built-in or plugin, flat out and counts what arrives, so a driver kept outside
//! this repo can be checked for lost samples the same way a built-in is.

use std::{
    fmt::Write as _,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::{DeviceError, DeviceRegistry, LaneEvent, RxSink, SinkItem, lock};

#[derive(Default)]
struct Counters {
    received: AtomicU64,
    reached: AtomicU64,
    gaps: AtomicU64,
    deaf: AtomicU64,
    failure: Mutex<Option<String>>,
}

/// What one bench run saw, summed over the radio's receive streams.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchReport {
    pub device: String,
    pub streams: u32,
    pub nominal_rate: Option<f64>,
    pub seconds: f64,
    pub received: u64,
    /// Samples the driver said it never delivered.
    pub lost: u64,
    /// Places the driver could not say exactly how much it lost.
    pub gaps: u64,
    pub failure: Option<String>,
}

impl BenchReport {
    #[must_use]
    pub fn rate(&self) -> f64 {
        if self.seconds <= 0.0 || self.streams == 0 {
            return 0.0;
        }
        self.received as f64 / self.seconds / f64::from(self.streams)
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!(
            "{}
",
            self.device
        );
        if let Some(rate) = self.nominal_rate {
            let _ = writeln!(out, "  set rate   {:.3} MS/s", rate / 1e6);
        }
        let _ = writeln!(
            out,
            "  got rate   {:.3} MS/s per stream over {:.1} s, {} stream(s)",
            self.rate() / 1e6,
            self.seconds,
            self.streams
        );
        let _ = writeln!(out, "  samples    {}", self.received);
        let _ = writeln!(out, "  lost       {}", self.lost);
        let _ = writeln!(out, "  gaps       {}", self.gaps);
        if let Some(failure) = &self.failure {
            let _ = writeln!(out, "  failed     {failure}");
        }
        out
    }
}

fn counting_sink(counters: &Arc<Counters>) -> RxSink {
    let seen = counters.clone();
    let failed = counters.clone();
    RxSink::with_items(
        move |item: SinkItem<'_>| match item {
            SinkItem::Samples { samples, index } => {
                seen.received
                    .fetch_add(samples.len() as u64, Ordering::Relaxed);
                seen.reached
                    .fetch_max(index + samples.len() as u64, Ordering::Relaxed);
            }
            SinkItem::Event(LaneEvent::Uncertain { .. }) => {
                seen.gaps.fetch_add(1, Ordering::Relaxed);
            }
            SinkItem::Event(LaneEvent::Deaf { samples, .. }) => {
                seen.deaf.fetch_add(samples, Ordering::Relaxed);
            }
            SinkItem::Event(_) => {}
        },
        move |error| *lock(&failed.failure) = Some(error.to_string()),
    )
}

/// Opens `device_id` from `registry`, streams every receive stream for `run` and reports.
pub fn bench_device(
    registry: &DeviceRegistry,
    device_id: &str,
    run: Duration,
) -> Result<BenchReport, DeviceError> {
    let (info, mut device) = registry.open(device_id)?;
    let streams = device.capabilities().rx_streams.max(1);
    let counters: Vec<Arc<Counters>> = (0..streams).map(|_| Arc::default()).collect();
    device.rx_start(counters.iter().map(counting_sink).collect())?;
    let start = Instant::now();
    while start.elapsed() < run && counters.iter().all(|c| lock(&c.failure).is_none()) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let seconds = start.elapsed().as_secs_f64();
    device.rx_stop();
    let total = |read: fn(&Counters) -> u64| counters.iter().map(|c| read(c)).sum::<u64>();
    let received = total(|c| c.received.load(Ordering::Relaxed));
    let reached = total(|c| c.reached.load(Ordering::Relaxed));
    let deaf = total(|c| c.deaf.load(Ordering::Relaxed));
    Ok(BenchReport {
        device: format!("{} [{}]", info.id(), info.label),
        streams,
        nominal_rate: device.settings().sample_rate,
        seconds,
        received,
        lost: reached.saturating_sub(received).saturating_sub(deaf),
        gaps: total(|c| c.gaps.load(Ordering::Relaxed)),
        failure: counters.iter().find_map(|c| lock(&c.failure).clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bench_counts_a_virtual_device() {
        let registry = crate::builtin_registry(None);
        let virtual_radio = registry
            .probe_all()
            .into_iter()
            .find(|info| info.driver == sdrmm_wire::DeviceBackend::VIRTUAL)
            .expect("a virtual radio is built in");
        let report = bench_device(&registry, &virtual_radio.id(), Duration::from_millis(300))
            .expect("the virtual radio benches");
        assert!(report.received > 0, "{report:?}");
        assert_eq!(report.lost, 0, "{report:?}");
        assert_eq!(report.failure, None);
        assert!(report.render().contains("lost       0"));
    }

    #[test]
    fn an_unknown_device_is_an_error() {
        let registry = crate::builtin_registry(None);
        assert!(matches!(
            bench_device(&registry, "nowhere:0", Duration::ZERO),
            Err(DeviceError::NotFound(_))
        ));
    }
}
