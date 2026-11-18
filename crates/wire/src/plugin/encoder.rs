//! The encoder half of the plugin ABI. A plugin whose manifest declares an `encoder` exports
//! these and turns payloads into baseband the way a built-in encoder does.
//!
//! Payloads travel in a buffer from `sdrmm_alloc`: audio as little-endian `f32` samples, a frame
//! as its bytes. `sdrmm_encoder_generate` leaves interleaved little-endian `f32` I/Q in a buffer the
//! guest keeps, and returns its pointer and sample count packed like every other reply. Its width
//! and its message forms go through `sdrmm_call` as [`super::Op::Width`] and
//! [`super::Op::Compose`].

/// Guest exports, by name.
pub mod export {
    /// `(settings_ptr: u32, settings_len: u32) -> handle: i32`
    pub const OPEN: &str = "sdrmm_encoder_open";
    /// `(handle: i32, kind: u32, ptr: u32, len: u32) -> i32`
    pub const SUBMIT: &str = "sdrmm_encoder_submit";
    /// `(handle: i32, max_samples: u32) -> packed (ptr, samples): i64`
    pub const GENERATE: &str = "sdrmm_encoder_generate";
    /// `(handle: i32)`
    pub const CLOSE: &str = "sdrmm_encoder_close";

    pub const ALL: [&str; 4] = [OPEN, SUBMIT, GENERATE, CLOSE];
}

/// What a submitted buffer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PayloadKind {
    Audio = 0,
    Frame = 1,
}

impl PayloadKind {
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        [Self::Audio, Self::Frame]
            .into_iter()
            .find(|kind| *kind as u32 == code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_kind_reads_back_from_its_code() {
        assert_eq!(PayloadKind::from_code(0), Some(PayloadKind::Audio));
        assert_eq!(PayloadKind::from_code(1), Some(PayloadKind::Frame));
        assert_eq!(PayloadKind::from_code(2), None);
    }
}
