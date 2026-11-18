use std::{borrow::Cow, sync::Arc};

use sdrmm_decoder::VideoPicture;

pub(crate) const IMAGE_CHANNEL_CAP: usize = 8;
pub(crate) const IMAGE_QUEUE_CAP: usize = 4;

#[derive(Clone, Debug)]
pub struct ImageCapture {
    pub device_set: u32,
    pub channel: u32,
    pub at: String,
    pub freq_hz: f64,
    pub source: Cow<'static, str>,
    pub mode: String,
    pub complete: bool,
    pub lines: u16,
    pub picture: Arc<VideoPicture>,
}
