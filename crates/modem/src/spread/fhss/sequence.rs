use super::super::pn::{PnError, PnSequence};

const MIN_CHANNELS: usize = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct HopSequence {
    channels: usize,
    spacing_cycles: f64,
    dwell_samples: usize,
    order: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FhssError {
    DegenerateSchedule,
    ChannelOutOfRange(usize),
    EmptyOrder,
    ExhaustedCode,
    Sequence(PnError),
}

impl std::fmt::Display for FhssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DegenerateSchedule => {
                write!(f, "a hop plan needs ≥ 2 channels and a dwell of ≥ 1 sample")
            }
            Self::ChannelOutOfRange(c) => write!(
                f,
                "the order names channel {c}, which the plan does not have"
            ),
            Self::EmptyOrder => write!(f, "an empty hop order visits nothing"),
            Self::ExhaustedCode => write!(
                f,
                "the code's period never drew enough in-range channels to fill the schedule"
            ),
            Self::Sequence(why) => write!(f, "hop sequence: {why}"),
        }
    }
}

impl std::error::Error for FhssError {}

impl HopSequence {
    pub fn new(
        channels: usize,
        spacing_cycles: f64,
        dwell_samples: usize,
        order: Vec<usize>,
    ) -> Result<Self, FhssError> {
        check_plan(channels, dwell_samples)?;
        if order.is_empty() {
            return Err(FhssError::EmptyOrder);
        }
        if let Some(&bad) = order.iter().find(|&&c| c >= channels) {
            return Err(FhssError::ChannelOutOfRange(bad));
        }
        Ok(Self {
            channels,
            spacing_cycles,
            dwell_samples,
            order,
        })
    }

    pub fn from_m_sequence(
        channels: usize,
        spacing_cycles: f64,
        dwell_samples: usize,
        hops: usize,
        degree: u32,
    ) -> Result<Self, FhssError> {
        check_plan(channels, dwell_samples)?;
        if hops == 0 {
            return Err(FhssError::EmptyOrder);
        }
        let code = PnSequence::maximal_length(degree).map_err(FhssError::Sequence)?;
        let bits = (usize::BITS - (channels - 1).leading_zeros()) as usize;
        let chips = code.chips();
        let budget = chips.len().saturating_mul(hops);
        let mut at = 0usize;
        let mut draws = 0usize;
        let mut order = Vec::with_capacity(hops);
        while order.len() < hops {
            if draws == budget {
                return Err(FhssError::ExhaustedCode);
            }
            draws += 1;
            let mut index = 0usize;
            for _ in 0..bits {
                index = (index << 1) | usize::from(chips[at % chips.len()] < 0.0);
                at += 1;
            }
            if index < channels {
                order.push(index);
            }
        }
        Self::new(channels, spacing_cycles, dwell_samples, order)
    }

    #[must_use]
    pub fn channels(&self) -> usize {
        self.channels
    }

    #[must_use]
    pub fn dwell_samples(&self) -> usize {
        self.dwell_samples
    }

    #[must_use]
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    #[must_use]
    pub fn spacing_cycles(&self) -> f64 {
        self.spacing_cycles
    }

    #[must_use]
    pub fn span_samples(&self) -> usize {
        self.order.len() * self.dwell_samples
    }

    #[must_use]
    pub fn channel(&self, hop: usize) -> usize {
        self.order[hop % self.order.len()]
    }

    #[must_use]
    pub fn offset_cycles(&self, channel: usize) -> f64 {
        (channel as f64 - (self.channels as f64 - 1.0) / 2.0) * self.spacing_cycles
    }

    #[must_use]
    pub fn visits(&self, hops: usize) -> usize {
        let mut seen = vec![false; self.channels];
        for hop in 0..hops {
            seen[self.channel(hop)] = true;
        }
        seen.iter().filter(|s| **s).count()
    }

    #[must_use]
    pub fn occupied_cycles(&self) -> f64 {
        (self.channels as f64 - 1.0) * self.spacing_cycles
    }
}

pub trait HopSequencer {
    fn channel(&self, hop: usize) -> usize;

    fn order(&self, hops: usize) -> Vec<usize> {
        (0..hops).map(|hop| self.channel(hop)).collect()
    }
}

impl HopSequencer for HopSequence {
    fn channel(&self, hop: usize) -> usize {
        Self::channel(self, hop)
    }
}

fn check_plan(channels: usize, dwell_samples: usize) -> Result<(), FhssError> {
    if channels < MIN_CHANNELS || dwell_samples == 0 {
        return Err(FhssError::DegenerateSchedule);
    }
    Ok(())
}
