//! Audio codecs the decoders share: AAC and xHE-AAC framing for DRM, D-STAR AMBE, G.726 ADPCM,
//! MPEG Layer II, and the MBE and Codec 2 vocoders with their optional hardware chip.

pub mod aac;
pub mod ambe;
pub mod g726;
pub mod mp2;
pub mod voice;

/// The rate every decoder here hands its audio out at.
pub const OUTPUT_RATE: u32 = 48_000;
