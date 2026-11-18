use std::fmt;

use num_complex::Complex;

use super::{
    BAND_RATE_HZ, Sink,
    band::{Band, BandError},
    channel_index,
    receiver::Lane,
    rf_channel_at,
};
use crate::cpm::LaurentError;

#[derive(Clone, Debug, PartialEq)]
pub enum FrontError {
    NotAChannel { frequency_hz: f64 },
    Lane(LaurentError),
    Band(BandError),
}

impl fmt::Display for FrontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAChannel { frequency_hz } => write!(
                f,
                "{:.3} MHz is not a Bluetooth channel: 2402 to 2480 MHz in 2 MHz steps",
                frequency_hz / 1e6
            ),
            Self::Lane(error) => write!(f, "{error}"),
            Self::Band(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FrontError {}

/// One channel at [`super::RATE_HZ`] or every channel in a [`BAND_RATE_HZ`] window.
pub enum Front {
    Channel(Box<Lane>),
    Band(Box<Band>),
}

impl Front {
    pub fn channel(frequency_hz: f64) -> Result<Self, FrontError> {
        let rf = rf_channel_at(frequency_hz).ok_or(FrontError::NotAChannel { frequency_hz })?;
        Ok(Self::Channel(Box::new(
            Lane::new(Some(rf), channel_index(rf)).map_err(FrontError::Lane)?,
        )))
    }

    pub fn band(frequency_hz: f64) -> Result<Self, FrontError> {
        Ok(Self::Band(Box::new(
            Band::new(BAND_RATE_HZ, frequency_hz).map_err(FrontError::Band)?,
        )))
    }

    pub fn reset(&mut self) {
        match self {
            Self::Channel(lane) => lane.reset(),
            Self::Band(band) => band.reset(),
        }
    }

    #[must_use]
    pub fn rejected(&self) -> u32 {
        match self {
            Self::Channel(lane) => lane.rejected(),
            Self::Band(band) => band.rejected(),
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        match self {
            Self::Channel(lane) => lane.process(iq, sink),
            Self::Band(band) => band.process(iq, sink),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_channel_centre_opens_a_single_channel_front() {
        assert!(Front::channel(2_426e6).is_ok());
        assert_eq!(
            Front::channel(2_427e6).err(),
            Some(FrontError::NotAChannel {
                frequency_hz: 2_427e6
            })
        );
        assert!(Front::band(2_440e6).is_ok());
    }
}
