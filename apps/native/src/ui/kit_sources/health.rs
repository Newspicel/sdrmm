use sdrmm_wire::{PipelineQueue, PipelineStage, state::DeviceSet};
use zgui::prelude::*;

use crate::{
    store::Store,
    ui::kit_audio::{self, Stat, drops_stat, stats},
};

#[derive(Clone, Debug, PartialEq)]
pub struct QueueSummary {
    pub oldest_ms: f64,
    pub dropped: u64,
    pub detail: String,
}

const fn stage_name(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::Capture => "capture",
        PipelineStage::Spectrum => "spectrum",
        PipelineStage::Channel => "channel",
    }
}

fn queue_line(queue: &PipelineQueue) -> String {
    let channel = queue
        .channel
        .map_or_else(String::new, |channel| format!("/{channel}"));
    format!(
        "{} {}{channel}: {}/{}, {:.1} ms, {} dropped",
        stage_name(queue.stage),
        queue.stream,
        queue.health.queued,
        queue.health.capacity,
        queue.health.oldest_ms,
        queue.health.dropped
    )
}

#[must_use]
pub fn queue_summary(queues: &[PipelineQueue], device_set: u32) -> Option<QueueSummary> {
    let mine: Vec<&PipelineQueue> = queues
        .iter()
        .filter(|queue| queue.device_set == device_set)
        .collect();
    if mine.is_empty() {
        return None;
    }
    Some(QueueSummary {
        oldest_ms: mine
            .iter()
            .map(|queue| queue.health.oldest_ms)
            .fold(f64::NEG_INFINITY, f64::max),
        dropped: mine.iter().map(|queue| queue.health.dropped).sum(),
        detail: mine
            .iter()
            .map(|queue| queue_line(queue))
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

#[must_use]
pub fn queue_stat(summary: &QueueSummary) -> Stat {
    Stat::new(
        "Queue",
        format!("{:.0} ms", summary.oldest_ms),
        summary.detail.clone(),
    )
}

#[must_use]
pub fn source_stats(queues: &[PipelineQueue], set: &DeviceSet) -> Vec<Stat> {
    let queue = queue_summary(queues, set.id).map(|summary| queue_stat(&summary));
    queue.into_iter().chain(drops_stat(set.overruns)).collect()
}

pub fn source_health(
    store: Store,
    set: Signal<Option<DeviceSet>>,
    extra: impl Fn(&DeviceSet) -> Vec<Stat> + 'static,
) -> impl IntoView {
    kit_audio::install();
    stats(move || {
        let queues = store.pipeline.get();
        set.with(|set| {
            set.as_ref().map_or_else(Vec::new, |set| {
                let mut shown = source_stats(&queues, set);
                shown.extend(extra(set));
                shown
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::QueueHealth;

    use super::*;

    fn queue(device_set: u32, channel: Option<u32>, oldest_ms: f64, dropped: u64) -> PipelineQueue {
        PipelineQueue {
            device_set,
            stream: 0,
            channel,
            stage: if channel.is_some() {
                PipelineStage::Channel
            } else {
                PipelineStage::Capture
            },
            health: QueueHealth {
                queued: 2,
                capacity: 8,
                oldest_ms,
                dropped,
            },
        }
    }

    #[test]
    fn a_source_reports_its_slowest_queue_and_every_drop() {
        let queues = [
            queue(1, None, 3.0, 1),
            queue(1, Some(4), 12.6, 2),
            queue(2, None, 99.0, 7),
        ];
        let summary = queue_summary(&queues, 1).expect("queues of set 1");
        assert_eq!(summary.oldest_ms, 12.6);
        assert_eq!(summary.dropped, 3);
        assert_eq!(
            summary.detail,
            "capture 0: 2/8, 3.0 ms, 1 dropped\nchannel 0/4: 2/8, 12.6 ms, 2 dropped"
        );
        assert!(queue_summary(&queues, 3).is_none());
        assert_eq!(queue_stat(&summary).value.as_deref(), Some("13 ms"));
    }
}
