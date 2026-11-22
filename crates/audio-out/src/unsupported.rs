#![expect(
    clippy::unused_self,
    clippy::unnecessary_wraps,
    reason = "the same shape as the platforms that play"
)]

use std::sync::Arc;

use crate::{
    Error, Result,
    mixer::{Mixer, Shared},
};

pub(crate) fn outputs() -> Result<Vec<String>> {
    Ok(Vec::new())
}

pub(crate) struct Device;

impl Device {
    pub(crate) fn find(_name: Option<&str>) -> Result<Self> {
        Err(Error::Unsupported)
    }

    pub(crate) const fn rate(&self) -> u32 {
        0
    }

    pub(crate) fn start(self, _mixer: Mixer, _shared: Arc<Shared>) -> Result<Stream> {
        Err(Error::Unsupported)
    }
}

pub(crate) struct Stream;

impl Stream {
    pub(crate) const fn fault(&self) -> Option<String> {
        None
    }
}
