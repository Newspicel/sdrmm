//! Folds interleaved multichannel audio down to one channel.

/// The mean of each frame's channels; a trailing partial frame is dropped.
pub fn mono_mix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_frames_average() {
        assert_eq!(mono_mix(&[0.5, -0.5, 1.0, 0.0], 2), [0.0, 0.5]);
    }

    #[test]
    fn mono_passes_through_even_with_no_channel_count() {
        assert_eq!(mono_mix(&[0.25, -0.75], 0), [0.25, -0.75]);
    }
}
