use std::sync::{Arc, Mutex};

use sdrmm_device::{DeviceError, lock};

use crate::{
    api::{DevHandle, Rtsa},
    model::{Kind, Mode},
};

/// The one handle a SPECTRAN is open under. The API opens a device for one mode at a time, so
/// receiving and transmitting each reopen it in theirs.
pub struct Link {
    pub api: Arc<dyn Rtsa>,
    pub kind: Kind,
    pub serial: String,
    open: Mutex<Option<(Mode, DevHandle)>>,
}

impl Link {
    pub fn open(
        api: Arc<dyn Rtsa>,
        kind: Kind,
        serial: &str,
        mode: Mode,
    ) -> Result<Self, DeviceError> {
        let dev = api.open(kind, mode, serial)?;
        Ok(Self {
            api,
            kind,
            serial: serial.to_string(),
            open: Mutex::new(Some((mode, dev))),
        })
    }

    #[must_use]
    pub fn current(&self) -> Option<(Mode, DevHandle)> {
        *lock(&self.open)
    }

    pub fn handle_in(&self, mode: Mode) -> Result<DevHandle, DeviceError> {
        match self.current() {
            Some((open, dev)) if open == mode => Ok(dev),
            _ => Err(DeviceError::Io(format!(
                "the SPECTRAN is not open as {}",
                mode.name()
            ))),
        }
    }

    /// The handle in `mode`, and whether it had to be reopened for it.
    pub fn switch(&self, mode: Mode) -> Result<(DevHandle, bool), DeviceError> {
        let mut open = lock(&self.open);
        if let Some((current, dev)) = *open {
            if current == mode {
                return Ok((dev, false));
            }
            if let Err(error) = self.api.close(dev) {
                tracing::warn!(%error, "closing the SPECTRAN before switching mode");
            }
            *open = None;
        }
        let dev = self.api.open(self.kind, mode, &self.serial)?;
        *open = Some((mode, dev));
        Ok((dev, true))
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some((_, dev)) = lock(&self.open).take()
            && let Err(error) = self.api.close(dev)
        {
            tracing::warn!(%error, "closing the SPECTRAN");
        }
    }
}
