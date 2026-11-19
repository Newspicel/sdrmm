//! What the one-minute JT modes share as channels: a decoder per slot and the events it emits.

use sdrmm_wire::{Capability, DecoderEvent, EventKind, Hue, KindSpec};

use super::{Decoded, Job, Mode, WsjtMessage, sync_tone::Heard};

/// A decoder of one 60-second slot of 12 kHz audio.
pub(super) trait MinuteDecoder: Send {
    fn decode(&mut self, samples: &[f32], low_hz: f32, high_hz: f32, limit: usize) -> Vec<Heard>;
}

pub(super) fn decode(
    job: &Job,
    decoder: &mut dyn MinuteDecoder,
    kind: &EventKind,
    mode: Mode,
) -> Vec<Decoded> {
    decoder
        .decode(
            &job.samples,
            job.audio_low_hz,
            job.audio_high_hz,
            job.max_candidates,
        )
        .into_iter()
        .map(|heard| {
            let (time_offset_s, slot) = mode.timing(job.window_time, heard.start_s);
            let message = WsjtMessage {
                text: heard.text,
                snr_db: heard.snr_db,
                audio_hz: heard.frequency_hz,
                time_offset_s,
                hard_errors: heard.hard_errors,
                slot,
            };
            Decoded {
                event: DecoderEvent::tagged(kind.clone(), &message),
                text: message.text,
                audio_hz: message.audio_hz,
                window_offset_s: heard.start_s - mode.nominal_start_s() as f32,
            }
        })
        .collect()
}

pub(super) fn kind(type_id: &'static str, label: &str) -> KindSpec {
    KindSpec::tagged::<WsjtMessage>(EventKind::named(type_id), label, "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

#[cfg(test)]
mod tests;
