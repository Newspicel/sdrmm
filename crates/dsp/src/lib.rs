pub mod array;
pub mod audio;
pub mod coding;
pub mod demod;
pub mod filter;
pub mod math;
pub mod measure;
pub mod radar;
pub mod scene;
pub mod tuning;

#[cfg(test)]
mod testutil;

pub use audio::agc::Agc;
pub use audio::compander::Compander;
pub use audio::noise::{AutoNotch, ClickRemover, NoiseBlanker, SpectralDenoiser};
pub use audio::squelch::Squelch;
pub use coding::bits::{
    Descrambler, DifferentialDecoder, HdlcDeframer, NrziDecoder, Scrambler, SyncDetector, bits_be,
    hamming_distance, manchester_decode, pack_lsb, pack_msb, pack_msb_into, reverse_byte, word_msb,
};
pub use coding::fec::{
    RdsOffset,
    block::{CyclicCode, ParityCode},
    bptc::{Bptc128, Bptc196},
    conv::{CONFIDENT, ERASURE, Soft, Viterbi5, soft},
    conv_soft::SoftViterbi,
    conv7::{ConvCode, Depuncturer, StreamViterbiK7, ViterbiK7, depuncture, puncture},
    crc_msb_bits, crc4_msb, crc8_msb, crc16_ccitt, crc16_msb, crc16_msb_bits, crc16_x25,
    crc24_reflected, crc32_ieee, crc32_mpeg, ermes_bch_decode, ermes_bch_encode, golay23_correct,
    golay23_encode, golay23_ok, hdlc_fcs_ok, hdlc_repair, lfsr_digest8, lfsr_digest8_reflect,
    lfsr_digest16, mode_s_append_overlaid_parity, mode_s_append_parity, mode_s_bit_overlays,
    mode_s_fix_single_bit, mode_s_overlay, mode_s_syndrome, pocsag_bch_decode, pocsag_bch_encode,
    prbs::{DAB_DISPERSAL, DVB_DISPERSAL, Prbs, PrbsSpec},
    rds_check_block, rds_correct_block, rds_encode_block, rds_syndrome, rs64_decode, rs64_encode,
    rs129_parity,
    rs256::{DVB_PRIMITIVE, ReedSolomon},
    syndrome::SyndromeDecoder,
};
pub use demod::fm::FmDemod;
pub use demod::ofdm::{CyclicPrefix, CyclicPrefixSearch};
pub use demod::pll::{Costas, LoopFilter, Pll};
pub use demod::sync::{BitSync, SymbolSync, farrow};
pub use demod::tone::{Envelope, Goertzel, KeyingSlicer, KeyingTiming, ToneCorrelator};
pub use filter::decim::{Decimator, RealDecimator};
pub use filter::fir::{
    design_bandpass, design_gaussian, design_halfband, design_lowpass,
    design_lowpass_blackman_harris, design_lowpass_kaiser, design_rds_biphase, design_rds_shaping,
    design_rrc, design_rrc_unit_energy,
};
pub use filter::firc::FirC;
pub use filter::iir::{
    Biquad, ComplexOnePole, DcBlocker, Deemphasis, Highpass, IqDcBlocker, one_pole_coeff,
};
pub use filter::interp::{CubicInterpolator, Interpolator, RealInterpolator};
pub use filter::moving::MovingSum;
pub use filter::real_resamp::RealResampler;
pub use filter::resamp::FracResampler;
pub use math::db::Decibels;
pub use math::fastmath::{fast_arg, fast_atan2, madd, phase_diff_into};
pub use math::window::{blackman_harris, coherent_gain, hann};
pub use measure::emission::EmissionMeter;
pub use measure::level::{LEVEL_FLOOR_DB, LevelMeter, power_db};
pub use measure::spectrum::{
    DbWindowSmoother, NoiseFloor, PowerAverage, SpanFloor, SpectrumAnalyzer, adaptive_db_window,
    decimate_signal, quantize_db,
};
pub use tuning::channelizer::{Channelizer, ChannelizerError};
pub use tuning::ddc::{Ddc, DdcError, flat_bandwidth_hz};
pub use tuning::duc::Duc;
pub use tuning::iq_notch::IqNotch;
pub use tuning::nco::Nco;
pub use tuning::realiq::RealToIq;
