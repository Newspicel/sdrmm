use crate::{
    convert::Format,
    discovery,
    error::{Error, Result},
};

mod audio;
mod hid;

pub(crate) use audio::{AudioStream as Stream, BLOCK_SAMPLES};
pub(crate) use hid::HidLink as Control;

pub(crate) const FORMAT: Format = Format::Float32;

pub(crate) fn open(key: &str) -> Result<(Control, Port)> {
    let location = discovery::location(key).ok_or(Error::DeviceNotFound)?;
    Ok((Control::open(location)?, Port { location }))
}

pub(crate) struct Port {
    location: u32,
}

impl Port {
    pub(crate) fn start(&self) -> Result<Stream> {
        audio::find_input(self.location).and_then(Stream::start)
    }
}
