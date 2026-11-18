mod array_hardware;
mod array_lifecycle;
mod array_plugin;
mod array_processors;
mod array_realtime;
mod array_record;
mod array_sync;
mod audio_record;
mod common;
mod decode;
#[cfg(feature = "rtlsdr")]
mod kraken;
mod listen;
mod multistream;
mod network_export;
mod radar_hardware;
mod record_replay;
mod sources;
mod transmit;
mod trunking;
mod video;

/// Every test in this binary runs against the built-in decoders and processors, installed once
/// before main.
#[ctor::ctor(unsafe)]
fn install_for_tests() {
    sdrmm_channels::builtin::install();
    sdrmm_array::builtin::install();
}
