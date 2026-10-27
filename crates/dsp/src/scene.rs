pub mod delay;
pub mod field;
pub mod rng;
pub mod world;

#[cfg(any(test, feature = "test-scene"))]
mod wave;

#[cfg(any(test, feature = "test-scene"))]
pub mod array;
#[cfg(any(test, feature = "test-scene"))]
pub mod echo;

#[cfg(any(test, feature = "test-scene"))]
pub use array::{
    ArrayScene, Distortion, HeadingTrack, SceneCopy, SceneError, SceneSignal, SceneSource,
};
pub use delay::{KAISER_BETA, KaiserSinc};
#[cfg(any(test, feature = "test-scene"))]
pub use echo::SceneEcho;
pub use field::{LaneRenderer, LaneSetup, amplitude};
pub use rng::{Draw, Rng, mix, mix_key, step};
pub use world::{
    Clutter, DONGLE_SPACING_M, Echo, Emitter, KRAKEN_RADIUS_M, LaneImpairments, MAX_BENCH_LANES,
    MAX_CLUTTER_DELAY_SAMPLES, MAX_CLUTTER_ECHOES, MAX_PPM, MAX_RIPPLE_CYCLES, MAX_RIPPLE_DB, Path,
    Pilot, ReportedGap, Ripple, Scene, Slip, Waveform, all_finite, circle, default_scene, line,
    uca_positions,
};
