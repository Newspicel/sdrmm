use sdrmm_wire::{PipelineQueue, PipelineStage, decode::BroadcastStatus};

use crate::decoders::detail::broadcast_detail;

pub const DROPS_HINT: &str = "Samples lost between the radio and here";
pub const REJECTED_HINT: &str = "Rejected: packets heard but failed their check";

#[derive(Clone, Debug, PartialEq)]
pub struct QueueSummary {
    pub oldest_ms: f64,
    pub dropped: u64,
    pub detail: String,
}

fn stage_name(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::Capture => "capture",
        PipelineStage::Spectrum => "spectrum",
        PipelineStage::Channel => "channel",
    }
}

fn queue_line(queue: &PipelineQueue) -> String {
    let lane = queue
        .channel
        .map_or_else(String::new, |channel| format!("/{channel}"));
    format!(
        "{} {}{lane}: {}/{}, {:.1} ms, {} dropped",
        stage_name(queue.stage),
        queue.stream,
        queue.health.queued,
        queue.health.capacity,
        queue.health.oldest_ms,
        queue.health.dropped
    )
}

#[must_use]
pub fn channel_queue_summary(
    queues: &[PipelineQueue],
    device_set: u32,
    channel: u32,
) -> Option<QueueSummary> {
    let mine: Vec<&PipelineQueue> = queues
        .iter()
        .filter(|queue| queue.device_set == device_set && queue.channel == Some(channel))
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
pub fn broadcast_title(status: &BroadcastStatus) -> String {
    broadcast_detail(status)
        .fields
        .iter()
        .map(|(label, value)| format!("{label}: {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Debug, PartialEq)]
pub struct BroadcastStat {
    pub label: &'static str,
    pub value: Option<String>,
    pub locked: bool,
}

#[must_use]
pub fn broadcast_stat(status: &BroadcastStatus) -> BroadcastStat {
    if status.locked {
        BroadcastStat {
            label: "SNR",
            value: Some(format!("{:.1} dB", status.snr_db)),
            locked: true,
        }
    } else {
        BroadcastStat {
            label: "No lock",
            value: None,
            locked: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::QueueHealth;

    use super::*;

    fn queue(channel: Option<u32>, oldest_ms: f64, dropped: u64) -> PipelineQueue {
        PipelineQueue {
            device_set: 1,
            stream: 0,
            channel,
            stage: PipelineStage::Channel,
            health: QueueHealth {
                queued: 2,
                capacity: 8,
                oldest_ms,
                dropped,
            },
        }
    }

    #[test]
    fn a_channel_summary_takes_its_own_queues_only() {
        let queues = vec![
            queue(Some(3), 12.0, 1),
            queue(Some(3), 40.0, 2),
            queue(Some(4), 99.0, 9),
            queue(None, 99.0, 9),
        ];
        let summary = channel_queue_summary(&queues, 1, 3).expect("a summary");
        assert_eq!(summary.oldest_ms, 40.0);
        assert_eq!(summary.dropped, 3);
        assert_eq!(
            summary.detail,
            "channel 0/3: 2/8, 12.0 ms, 1 dropped\nchannel 0/3: 2/8, 40.0 ms, 2 dropped"
        );
        assert_eq!(channel_queue_summary(&queues, 2, 3), None);
        assert_eq!(channel_queue_summary(&[], 1, 3), None);
    }

    #[test]
    fn a_broadcast_reads_snr_once_locked() {
        let mut status = BroadcastStatus {
            snr_db: 14.26,
            ..BroadcastStatus::default()
        };
        assert_eq!(
            broadcast_stat(&status),
            BroadcastStat {
                label: "No lock",
                value: None,
                locked: false
            }
        );
        status.locked = true;
        assert_eq!(broadcast_stat(&status).value.as_deref(), Some("14.3 dB"));
        assert!(broadcast_title(&status).contains("SNR: 14.3 dB"));
    }
}
