//! DVSI AMBE-3000R and AMBE-3003 vocoder chips on a USB serial port: ThumbDV, DV3000U,
//! USB-3000, USB-3003. Each port gets one worker thread that leases the chip's channels to
//! decoders while they have a call.

mod chip;
pub mod protocol;
mod transport;
mod worker;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

pub use chip::{BAUDS, Chip, ChipInfo};
use sdrmm_channels::chip::{ChipLink, ChipRate, VocoderChips, chip_link};
pub use transport::{Opener, SerialOpener, Transport};
use worker::{Dongle, Dongles, Worker};

#[derive(Debug, thiserror::Error)]
pub enum ChipError {
    #[error("cannot open: {0}")]
    Open(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("no DVSI chip answered: {0}")]
    Silent(String),
    #[error("unexpected answer: {0}")]
    Protocol(String),
    #[error("chip refused field {field:#04x} with status {status}")]
    Refused { field: u8, status: u8 },
}

/// Every chip this process has open, by port.
pub struct Chips {
    opener: Arc<dyn Opener>,
    dongles: Dongles,
    next_id: AtomicU64,
}

impl Chips {
    #[must_use]
    pub fn new() -> Self {
        Self::with_opener(Arc::new(SerialOpener))
    }

    #[must_use]
    pub fn with_opener(opener: Arc<dyn Opener>) -> Self {
        Self {
            opener,
            dongles: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(0),
        }
    }

    fn start(&self, port: &str) -> Result<Dongle, ChipError> {
        let chip = Chip::probe(self.opener.as_ref(), port, &BAUDS)?;
        let info = chip.info();
        tracing::info!(
            port,
            product = info.product,
            version = info.version,
            baud = info.baud,
            channels = info.channels,
            "dvsi chip found"
        );
        let name: Arc<str> = info.product.as_str().into();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (attach, attached) = mpsc::channel();
        let worker = Worker::new(chip, port, id, Arc::clone(&self.opener), attached);
        let dongles = Arc::clone(&self.dongles);
        std::thread::Builder::new()
            .name(format!("dvsi {port}"))
            .spawn(move || worker.run(&dongles))?;
        Ok(Dongle { attach, name, id })
    }
}

impl Default for Chips {
    fn default() -> Self {
        Self::new()
    }
}

impl VocoderChips for Chips {
    fn open(&self, port: &str, rate: ChipRate) -> Result<ChipLink, String> {
        let mut dongles = self.dongles.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(dongle) = dongles.get(port) {
            let (link, end) = chip_link(rate, &dongle.name);
            if dongle.attach.send(end).is_ok() {
                return Ok(link);
            }
            dongles.remove(port);
        }
        let dongle = self.start(port).map_err(|error| error.to_string())?;
        let (link, end) = chip_link(rate, &dongle.name);
        dongle
            .attach
            .send(end)
            .map_err(|_| "the chip worker stopped at once".to_owned())?;
        dongles.insert(port.to_owned(), dongle);
        Ok(link)
    }
}
