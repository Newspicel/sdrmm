use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::account::MAX_ACCOUNT_LEN;

pub const DEFAULT_VOX_THRESHOLD_DB: f64 = -35.0;
pub const MIN_VOX_THRESHOLD_DB: f64 = -90.0;
pub const MAX_VOX_THRESHOLD_DB: f64 = 0.0;
pub const DEFAULT_VOX_HANG_MS: u32 = 600;
pub const MAX_VOX_HANG_MS: u32 = 5_000;
pub const DEFAULT_VOICE_VOLUME: f64 = 1.0;
pub const MIN_VOICE_VOLUME: f64 = 0.0;
pub const MAX_VOICE_VOLUME: f64 = 2.0;
pub const MAX_CHAT_CHANNEL_LEN: usize = 255;
pub const MAX_CHAT_ALLOWED: usize = 64;
pub const MAX_CHAT_ALLOWED_LEN: usize = 64;

/// How a voice source decides when it is sending.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    #[default]
    PushToTalk,
    VoiceActivity,
    Continuous,
}

/// The voice gate of one direction: the mode, the level that opens it, how long it holds and
/// the linear gain applied to what it lets through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct VoiceGateSettings {
    pub mode: SendMode,
    pub threshold_db: f64,
    pub hang_ms: u32,
    pub volume: f64,
}

impl Default for VoiceGateSettings {
    fn default() -> Self {
        Self {
            mode: SendMode::VoiceActivity,
            threshold_db: DEFAULT_VOX_THRESHOLD_DB,
            hang_ms: DEFAULT_VOX_HANG_MS,
            volume: DEFAULT_VOICE_VOLUME,
        }
    }
}

impl VoiceGateSettings {
    #[must_use]
    pub fn valid(&self) -> bool {
        (MIN_VOX_THRESHOLD_DB..=MAX_VOX_THRESHOLD_DB).contains(&self.threshold_db)
            && self.hang_ms <= MAX_VOX_HANG_MS
            && (MIN_VOICE_VOLUME..=MAX_VOICE_VOLUME).contains(&self.volume)
    }
}

/// Voice chat joins one channel of a chat account and trades audio with the patch both ways.
/// Push to talk needs a key a server cannot hold, so neither direction accepts it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct VoiceChatNode {
    pub account: String,
    pub channel: String,
    /// Chat users whose voice goes to the air. Empty lets anyone through.
    pub allowed: Vec<String>,
    pub to_air: VoiceGateSettings,
    pub to_chat: VoiceGateSettings,
}

impl VoiceChatNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.account.chars().count() <= MAX_ACCOUNT_LEN
            && self.channel.chars().count() <= MAX_CHAT_CHANNEL_LEN
            && self.allowed.len() <= MAX_CHAT_ALLOWED
            && self
                .allowed
                .iter()
                .all(|user| user.chars().count() <= MAX_CHAT_ALLOWED_LEN)
            && [&self.to_air, &self.to_chat]
                .into_iter()
                .all(|gate| gate.valid() && gate.mode != SendMode::PushToTalk)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceChatState {
    #[default]
    Idle,
    Connecting,
    Joined,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChatTalker {
    pub id: String,
    pub name: String,
    pub allowed: bool,
    /// False while the chat has not tied the voice to a user: `id` is a stand-in.
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VoiceChatStatus {
    pub node: String,
    pub state: VoiceChatState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub talking: Vec<ChatTalker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_chat_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_air_db: Option<f32>,
    #[serde(default)]
    pub to_chat_open: bool,
    #[serde(default)]
    pub to_air_open: bool,
}

#[cfg(test)]
mod tests;
