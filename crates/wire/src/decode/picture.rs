use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::channel::SstvMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SstvPicture {
    pub seq: u32,
    pub mode: SstvMode,
    pub width: u16,
    pub height: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
}
