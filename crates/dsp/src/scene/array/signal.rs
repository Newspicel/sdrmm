#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SceneSignal {
    Tone {
        offset_hz: f64,
    },
    Noise {
        offset_hz: f64,
        bandwidth_hz: f64,
    },
    Fm {
        offset_hz: f64,
        deviation_hz: f64,
        rate_hz: f64,
    },
    NoiseFm {
        offset_hz: f64,
        deviation_hz: f64,
        bandwidth_hz: f64,
    },
    Broadband,
}

impl SceneSignal {
    #[must_use]
    pub const fn offset_hz(self) -> f64 {
        match self {
            Self::Tone { offset_hz }
            | Self::Noise { offset_hz, .. }
            | Self::Fm { offset_hz, .. }
            | Self::NoiseFm { offset_hz, .. } => offset_hz,
            Self::Broadband => 0.0,
        }
    }

    pub(in crate::scene) const fn is_random(self) -> bool {
        matches!(
            self,
            Self::Noise { .. } | Self::NoiseFm { .. } | Self::Broadband
        )
    }

    pub(super) fn is_valid(self) -> bool {
        match self {
            Self::Tone { offset_hz } => offset_hz.is_finite(),
            Self::Noise {
                offset_hz,
                bandwidth_hz,
            } => offset_hz.is_finite() && bandwidth_hz.is_finite() && bandwidth_hz > 0.0,
            Self::Fm {
                offset_hz,
                deviation_hz,
                rate_hz,
            } => offset_hz.is_finite() && deviation_hz.is_finite() && rate_hz.is_finite(),
            Self::NoiseFm {
                offset_hz,
                deviation_hz,
                bandwidth_hz,
            } => {
                offset_hz.is_finite()
                    && deviation_hz.is_finite()
                    && bandwidth_hz.is_finite()
                    && bandwidth_hz > 0.0
            }
            Self::Broadband => true,
        }
    }
}
