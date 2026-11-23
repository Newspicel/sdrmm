//! The Stream out node: a channel's pictures and sound as an MPEG transport stream.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MAX_STREAM_ADDRESS_LEN: usize = 255;

/// How the stream leaves: served to players that ask, or sent to one address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamTransport {
    #[default]
    Http,
    Udp,
    Rtp,
}

impl StreamTransport {
    /// Whether the stream goes to the node's address rather than to players that ask.
    #[must_use]
    pub const fn sends(self) -> bool {
        !matches!(self, Self::Http)
    }
}

fn default_stream_address() -> String {
    "127.0.0.1:5004".to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct StreamOutNode {
    pub transport: StreamTransport,
    /// Where UDP and RTP go, as `host:port`.
    pub address: String,
}

impl Default for StreamOutNode {
    fn default() -> Self {
        Self {
            transport: StreamTransport::Http,
            address: default_stream_address(),
        }
    }
}

impl StreamOutNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        !self.transport.sends()
            || (!self.address.is_empty() && self.address.len() <= MAX_STREAM_ADDRESS_LEN)
    }
}

/// Where a node serves its stream over HTTP.
#[must_use]
pub fn stream_out_path(node: &str) -> String {
    format!("/api/streams/{node}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StreamOutStatus {
    pub node: String,
    /// The path players open, while the node serves over HTTP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub clients: u32,
    pub pictures: u64,
    pub width: u16,
    pub height: u16,
    pub bytes: u64,
    /// Pictures and sound the encoder fell too far behind to take.
    pub dropped: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sent_stream_needs_an_address() {
        assert!(StreamOutNode::default().valid());
        let bare = StreamOutNode {
            transport: StreamTransport::Udp,
            address: String::new(),
        };
        assert!(!bare.valid());
        assert!(
            StreamOutNode {
                transport: StreamTransport::Http,
                ..bare
            }
            .valid()
        );
        assert_eq!(
            serde_json::from_str::<StreamOutNode>("{}").unwrap(),
            StreamOutNode::default()
        );
    }
}
