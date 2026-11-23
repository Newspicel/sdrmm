//! What the call loop needs of a `LiveKit` connection, so tests can stand in for one.

use async_trait::async_trait;

use crate::matrix::focus::LiveKitGrant;
use crate::matrix::livekit::peer::OpusOut;
use crate::matrix::membership::Focus;

/// What a wire tells the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WireEvent {
    /// One Opus frame, sealed in an encrypted room, from a `LiveKit` identity.
    Heard { identity: String, payload: Vec<u8> },
    /// A `LiveKit` identity left the room.
    Left(String),
}

/// Whether a connection publishes our mic or only listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Talk { encrypted: bool },
    Listen,
}

/// One `LiveKit` connection: signalling and its peers.
#[async_trait]
pub(crate) trait Wire: Send {
    /// Queues a frame on our mic track. A wire that only listens drops it.
    fn publish(&mut self, frame: OpusOut);

    /// The next event. Safe to cancel.
    async fn next(&mut self) -> Result<WireEvent, String>;

    /// Heard frames lost inside the wire so far.
    fn dropped(&self) -> u64;

    async fn leave(&mut self);
}

/// Opens wires.
#[async_trait]
pub(crate) trait Dial: Sync {
    async fn grant(&self, focus: &Focus) -> Result<LiveKitGrant, String>;

    async fn connect(&self, grant: &LiveKitGrant, role: Role) -> Result<Box<dyn Wire>, String>;
}
