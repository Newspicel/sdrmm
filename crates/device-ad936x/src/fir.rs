use sdrmm_dsp::fir::{design_lowpass_kaiser, kaiser_transition};

pub(crate) const FIR_CONFIG: &str = "filter_fir_config";
pub(crate) const FIR_CHANNEL: &str = "out";
pub(crate) const FIR_ENABLE: &str = "voltage_filter_fir_en";

pub(crate) const SAFE_RATE_HZ: f64 = 3_000_000.0;
const LOW_RATE_HZ: f64 = 25_000_000.0 / 12.0;
const QUAD_DECIMATION_MAX_HZ: f64 = 20_000_000.0;
const FULL_TAPS_MAX_HZ: f64 = 40_000_000.0;
const MID_TAPS_MAX_HZ: f64 = 160_000_000.0 / 3.0;
const ATTENUATION_DB: f64 = 72.0;
const UNITY: f64 = 32_768.0;
const DESIGNS: [(f64, u32, usize); 4] = [
    (QUAD_DECIMATION_MAX_HZ, 4, 128),
    (FULL_TAPS_MAX_HZ, 2, 128),
    (MID_TAPS_MAX_HZ, 2, 96),
    (f64::INFINITY, 2, 64),
];
const TX_GAIN: f64 = 2.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fir {
    pub(crate) decimation: u32,
    pub(crate) rx: Vec<i16>,
    pub(crate) tx: Vec<i16>,
}

impl Fir {
    pub(crate) fn for_rate(rate_hz: f64) -> Self {
        let (decimation, taps) = design(rate_hz);
        let shape = design_lowpass_kaiser(taps, 0.5 / f64::from(decimation), ATTENUATION_DB);
        let peak = shape
            .iter()
            .fold(0.0f64, |peak, &tap| peak.max(f64::from(tap).abs()));
        let tx_gain = TX_GAIN.min(f64::from(i16::MAX) / (peak * UNITY));
        let quantize = |gain: f64| -> Vec<i16> {
            shape
                .iter()
                .map(|&tap| (f64::from(tap) * UNITY * gain).round() as i16)
                .collect()
        };
        Self {
            decimation,
            rx: quantize(1.0),
            tx: quantize(tx_gain),
        }
    }

    pub(crate) fn config(&self) -> String {
        let header = format!(
            "RX 3 GAIN 0 DEC {0}\nTX 3 GAIN 0 INT {0}\n",
            self.decimation
        );
        let taps = self
            .rx
            .iter()
            .zip(&self.tx)
            .map(|(rx, tx)| format!("{rx},{tx}\n"));
        std::iter::once(header).chain(taps).collect()
    }
}

fn design(rate_hz: f64) -> (u32, usize) {
    DESIGNS
        .iter()
        .find(|(max_hz, ..)| rate_hz <= *max_hz)
        .map_or((2, 64), |&(_, decimation, taps)| (decimation, taps))
}

pub(crate) fn clean_fraction() -> f64 {
    DESIGNS
        .iter()
        .map(|&(_, decimation, taps)| {
            1.0 - f64::from(decimation) * kaiser_transition(taps, ATTENUATION_DB)
        })
        .fold(1.0, f64::min)
}

pub(crate) fn needs_safe_rate(rate_hz: f64) -> bool {
    rate_hz <= LOW_RATE_HZ
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_db(taps: &[i16], normalized_hz: f64) -> f64 {
        let (re, im) = taps
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(re, im), (n, &tap)| {
                let phase = -std::f64::consts::TAU * normalized_hz * n as f64;
                (
                    re + f64::from(tap) * phase.cos(),
                    im + f64::from(tap) * phase.sin(),
                )
            });
        20.0 * ((re * re + im * im).sqrt() / UNITY).log10()
    }

    #[test]
    fn up_to_20_msps_the_filter_decimates_by_four_with_128_taps() {
        let fir = Fir::for_rate(10e6);
        assert_eq!(fir.decimation, 4);
        assert_eq!(fir.rx.len(), 128);
        assert_eq!(Fir::for_rate(30.72e6).decimation, 2);
        assert_eq!(Fir::for_rate(50e6).rx.len(), 96);
        assert_eq!(Fir::for_rate(61.44e6).rx.len(), 64);
    }

    #[test]
    fn the_passband_is_flat_and_the_alias_band_is_deep() {
        for rate in [10e6, 30.72e6] {
            let fir = Fir::for_rate(rate);
            let input = rate * f64::from(fir.decimation);
            for hz in [0.0, 0.2 * rate, 0.4 * rate] {
                assert!(response_db(&fir.rx, hz / input).abs() < 0.1, "{rate} {hz}");
            }
            for hz in [0.6 * rate, 0.75 * rate, 0.95 * rate] {
                assert!(response_db(&fir.rx, hz / input) < -55.0, "{rate} {hz}");
            }
        }
    }

    #[test]
    fn nothing_folds_into_the_clean_fraction() {
        let clean = clean_fraction();
        assert!((0.8..0.9).contains(&clean), "{clean}");
        for rate in [10e6, 30.72e6, 50e6, 61.44e6] {
            let fir = Fir::for_rate(rate);
            let input = rate * f64::from(fir.decimation);
            let fold = (1.0 - clean / 2.0) * rate;
            for step in 0..50 {
                let hz = fold + (input / 2.0 - fold) * f64::from(step) / 50.0;
                let db = response_db(&fir.rx, hz / input);
                assert!(db < -60.0, "{rate}: {hz} at {db:.1} dB");
            }
        }
    }

    #[test]
    fn transmit_taps_stay_inside_sixteen_bits() {
        for rate in [10e6, 30.72e6, 50e6, 61.44e6] {
            let fir = Fir::for_rate(rate);
            let peak = fir.tx.iter().map(|tap| tap.unsigned_abs()).max();
            assert!(peak.is_some_and(|peak| peak <= i16::MAX.unsigned_abs()));
            assert!(
                fir.tx
                    .iter()
                    .zip(&fir.rx)
                    .any(|(tx, rx)| tx.abs() > rx.abs())
            );
        }
    }

    #[test]
    fn the_config_names_both_paths_and_lists_every_tap() {
        let fir = Fir::for_rate(10e6);
        let config = fir.config();
        let mut lines = config.lines();
        assert_eq!(lines.next(), Some("RX 3 GAIN 0 DEC 4"));
        assert_eq!(lines.next(), Some("TX 3 GAIN 0 INT 4"));
        assert_eq!(lines.count(), 128);
    }
}
