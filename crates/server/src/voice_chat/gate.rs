use sdrmm_dsp::audio::voice_gate::VoiceGate;
use sdrmm_wire::{SendMode, VoiceGateSettings};

pub(super) fn gate(settings: &VoiceGateSettings) -> VoiceGate {
    VoiceGate::new(settings.threshold_db as f32, settings.hang_ms)
}

/// Applies the direction's linear gain to a frame the gate let through, keeping it in range.
pub(super) fn scaled(frame: &[f32], settings: &VoiceGateSettings) -> Vec<f32> {
    let volume = settings.volume as f32;
    frame
        .iter()
        .map(|sample| (sample * volume).clamp(-1.0, 1.0))
        .collect()
}

/// Runs a frame through the gate. Continuous sends every frame, push to talk none, since a
/// server holds no key.
pub(super) fn passes(gate: &mut VoiceGate, settings: &VoiceGateSettings, frame: &[f32]) -> bool {
    gate.set(settings.threshold_db as f32, settings.hang_ms);
    let voiced = gate.feed(frame);
    match settings.mode {
        SendMode::Continuous => true,
        SendMode::VoiceActivity => voiced,
        SendMode::PushToTalk => false,
    }
}
