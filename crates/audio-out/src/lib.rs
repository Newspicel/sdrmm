//! Plays audio on the server's own sound card. Each output mixes any number of voices, each fed
//! from its own thread, so a speaker with several wires opens its device once.

#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    expect(dead_code, reason = "only a platform that plays drains the mixer")
)]
mod mixer;

#[cfg(target_os = "linux")]
mod alsa;
#[cfg(target_os = "macos")]
mod core_audio;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

#[cfg(target_os = "linux")]
use alsa as platform;
#[cfg(target_os = "macos")]
use core_audio as platform;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use unsupported as platform;

use std::sync::Arc;

pub use mixer::Voice;
use mixer::{Desk, Shared};

/// Interleaved stereo is what every voice carries; the platform maps it onto the device.
pub const CHANNELS: usize = 2;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not connected")]
    NotFound(String),
    #[error("{operation} failed ({status})")]
    Platform {
        operation: &'static str,
        status: i32,
    },
    #[error("{0}")]
    Backend(String),
    #[error("This server cannot play audio")]
    Unsupported,
    #[error("Too many inputs on one output")]
    Full,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Every output device the server can play on, by the name its operating system gives it.
pub fn outputs() -> Result<Vec<String>> {
    platform::outputs()
}

/// An open output device. Dropping it stops the device.
pub struct Output {
    desk: Desk,
    shared: Arc<Shared>,
    stream: platform::Stream,
    rate: u32,
}

impl Output {
    /// Opens a device by name, or the system default for `None`.
    pub fn open(name: Option<&str>) -> Result<Self> {
        let device = platform::Device::find(name)?;
        let rate = device.rate();
        let shared = Arc::new(Shared::default());
        let (desk, mixer) = mixer::pair(rate, shared.clone());
        let stream = device.start(mixer, shared.clone())?;
        Ok(Self {
            desk,
            shared,
            stream,
            rate,
        })
    }

    /// Frames per second each voice must carry.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        self.rate
    }

    pub fn voice(&mut self) -> Result<Voice> {
        self.desk.voice()
    }

    /// Why the device stopped playing, once it has.
    #[must_use]
    pub fn fault(&self) -> Option<String> {
        self.shared.fault().or_else(|| self.stream.fault())
    }

    /// Times a voice ran dry and had to build its buffer up again.
    #[must_use]
    pub fn underruns(&self) -> u64 {
        self.shared.underruns()
    }
}
