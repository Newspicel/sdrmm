use sdrmm_wire::IsmCoding;

use super::{RATE, bits::Bits, pulse::Package};

const SAMPLES_PER_US: f64 = RATE / 1e6;
const PCM_PREAMBLE_BITS: usize = 12;
const PCM_MEASURED_BITS: u32 = 20;

#[derive(Clone, Copy, Debug)]
pub(super) struct Timing {
    pub(super) short_us: u32,
    pub(super) long_us: u32,
    pub(super) gap_us: u32,
    pub(super) reset_us: u32,
    pub(super) sync_us: u32,
    pub(super) tolerance_us: u32,
}

impl Timing {
    pub(super) const fn new(short_us: u32, long_us: u32, reset_us: u32) -> Self {
        Self {
            short_us,
            long_us,
            gap_us: 0,
            reset_us,
            sync_us: 0,
            tolerance_us: 0,
        }
    }

    pub(super) const fn gap(self, gap_us: u32) -> Self {
        Self { gap_us, ..self }
    }

    pub(super) const fn sync(self, sync_us: u32) -> Self {
        Self { sync_us, ..self }
    }

    pub(super) const fn tolerance(self, tolerance_us: u32) -> Self {
        Self {
            tolerance_us,
            ..self
        }
    }

    fn samples(self) -> Samples {
        let scale = |us: u32| (f64::from(us) * SAMPLES_PER_US) as i64;
        Samples {
            short: scale(self.short_us),
            long: scale(self.long_us),
            gap: scale(self.gap_us),
            reset: scale(self.reset_us),
            sync: scale(self.sync_us),
            tolerance: scale(self.tolerance_us),
        }
    }
}

#[derive(Clone, Copy)]
struct Samples {
    short: i64,
    long: i64,
    gap: i64,
    reset: i64,
    sync: i64,
    tolerance: i64,
}

#[derive(Clone, Copy)]
struct Window {
    above: i64,
    below: i64,
}

impl Window {
    const NEVER: Self = Self { above: 0, below: 0 };

    fn around(centre: i64, tolerance: i64) -> Self {
        Self {
            above: centre - tolerance,
            below: centre + tolerance,
        }
    }

    fn holds(self, value: i64) -> bool {
        value > self.above && value < self.below
    }
}

pub(super) fn slice(
    coding: IsmCoding,
    timing: Timing,
    package: &Package<'_>,
    bits: &mut Bits,
    emit: &mut dyn FnMut(&mut Bits),
) {
    bits.clear();
    let samples = timing.samples();
    match coding {
        IsmCoding::Pwm => pwm(samples, package, bits, emit),
        IsmCoding::Ppm => ppm(samples, package, bits, emit),
        IsmCoding::Pcm => pcm(samples, package, bits, emit),
        IsmCoding::Manchester => manchester(samples, package, bits, emit),
    }
}

fn pwm_windows(s: Samples) -> (Window, Window, Window) {
    let mid = |a: i64, b: i64| (a + b) / 2 + 1;
    if s.tolerance > 0 {
        let sync = if s.sync > 0 {
            Window::around(s.sync, s.tolerance)
        } else {
            Window::NEVER
        };
        return (
            Window::around(s.short, s.tolerance),
            Window::around(s.long, s.tolerance),
            sync,
        );
    }
    let open = |above: i64, below: i64| Window { above, below };
    if s.sync <= 0 {
        let one = open(0, mid(s.short, s.long));
        (one, open(one.below - 1, i64::MAX), Window::NEVER)
    } else if s.sync < s.short {
        let sync = open(0, mid(s.sync, s.short));
        let one = open(sync.below - 1, mid(s.short, s.long));
        (one, open(one.below - 1, i64::MAX), sync)
    } else if s.sync < s.long {
        let one = open(0, mid(s.short, s.sync));
        let sync = open(one.below - 1, mid(s.sync, s.long));
        (one, open(sync.below - 1, i64::MAX), sync)
    } else {
        let one = open(0, mid(s.short, s.long));
        let zero = open(one.below - 1, mid(s.long, s.sync));
        (one, zero, open(zero.below - 1, i64::MAX))
    }
}

fn pwm(s: Samples, package: &Package<'_>, bits: &mut Bits, emit: &mut dyn FnMut(&mut Bits)) {
    let (one, zero, sync) = pwm_windows(s);
    let count = package.len();
    for n in 0..count {
        let (pulse, gap) = (i64::from(package.pulses[n]), i64::from(package.gaps[n]));
        if one.holds(pulse) {
            bits.push(true);
        } else if zero.holds(pulse) {
            bits.push(false);
        } else if sync.holds(pulse) {
            bits.sync();
        } else if pulse > one.above {
            bits.new_row();
        }
        if (n + 1 == count || gap > s.reset) && bits.rows() > 0 {
            emit(bits);
            bits.clear();
        } else if s.gap > 0 && gap > s.gap && bits.rows() > 0 && bits.len(bits.rows() - 1) > 0 {
            bits.new_row();
        }
    }
}

fn ppm(s: Samples, package: &Package<'_>, bits: &mut Bits, emit: &mut dyn FnMut(&mut Bits)) {
    let (zero, one, sync) = if s.tolerance > 0 {
        let sync = if s.sync > 0 {
            Window::around(s.sync, s.tolerance)
        } else {
            Window::NEVER
        };
        (
            Window::around(s.short, s.tolerance),
            Window::around(s.long, s.tolerance),
            sync,
        )
    } else {
        let split = (s.short + s.long) / 2 + 1;
        let top = if s.gap > 0 { s.gap } else { s.reset };
        (
            Window {
                above: 0,
                below: split,
            },
            Window {
                above: split - 1,
                below: top,
            },
            Window::NEVER,
        )
    };
    let count = package.len();
    for n in 0..count {
        let gap = i64::from(package.gaps[n]);
        if zero.holds(gap) {
            bits.push(false);
        } else if one.holds(gap) {
            bits.push(true);
        } else if sync.holds(gap) {
            bits.sync();
        } else if gap < s.reset {
            bits.new_row();
        }
        if (n + 1 == count || gap >= s.reset) && has_data(bits) {
            emit(bits);
            bits.clear();
        }
    }
}

fn has_data(bits: &Bits) -> bool {
    bits.rows() > 1 || (bits.rows() == 1 && bits.len(0) > 0)
}

fn pcm_rate(s: Samples, package: &Package<'_>) -> f64 {
    let tolerance = if s.tolerance > 0 {
        s.tolerance
    } else {
        s.long / 4
    };
    let base = 1.0 / s.short.max(1) as f64;
    preamble_rate(package, base).unwrap_or_else(|| measured_rate(s.short, tolerance, package, base))
}

fn preamble_rate(package: &Package<'_>, base: f64) -> Option<f64> {
    let unit = |width: u32| (f64::from(width) * base + 0.5) as i64 == 1;
    let mut best: Option<(usize, f64)> = None;
    let mut n = 0;
    while n < package.len() {
        let (mut width, mut count) = (0u64, 0usize);
        while n < package.len() && unit(package.pulses[n]) && unit(package.gaps[n]) {
            width += u64::from(package.pulses[n]) + u64::from(package.gaps[n]);
            count += 2;
            n += 1;
        }
        let needed = best.map_or(PCM_PREAMBLE_BITS, |(count, _)| count);
        if count >= needed && width > 0 {
            best = Some((count, count as f64 / width as f64));
        }
        n += 1;
    }
    best.map(|(_, rate)| rate)
}

fn measured_rate(short: i64, tolerance: i64, package: &Package<'_>, base: f64) -> f64 {
    let (mut width, mut count) = (0i64, 0u32);
    for value in package.pulses.iter().chain(package.gaps) {
        let value = i64::from(*value);
        if (value - short).abs() <= tolerance {
            width += value;
            count += 1;
        } else if (value - 2 * short).abs() <= tolerance {
            width += value;
            count += 2;
        }
    }
    if count > PCM_MEASURED_BITS {
        f64::from(count) / width as f64
    } else {
        base
    }
}

fn pcm(s: Samples, package: &Package<'_>, bits: &mut Bits, emit: &mut dyn FnMut(&mut Bits)) {
    let rate = pcm_rate(s, package);
    let gap_limit = if s.gap > 0 { s.gap } else { s.reset };
    let max_zeros = gap_limit / s.long.max(1);
    let count = package.len();
    for n in 0..count {
        let (pulse, gap) = (package.pulses[n], i64::from(package.gaps[n]));
        let highs = (f64::from(pulse) * rate + 0.5) as i64;
        let lows = ((gap as f64 * rate + 0.5) as i64).min(max_zeros);
        (0..highs).for_each(|_| bits.push(true));
        (0..lows).for_each(|_| bits.push(false));
        if gap > gap_limit && gap <= s.reset {
            bits.new_row();
        }
        if (n + 1 == count || gap > s.reset) && has_data(bits) {
            emit(bits);
            bits.clear();
        }
    }
}

fn manchester(s: Samples, package: &Package<'_>, bits: &mut Bits, emit: &mut dyn FnMut(&mut Bits)) {
    let edge = s.short as f64 * 1.5;
    let (low, high) = (s.short - s.tolerance, 2 * s.short + s.tolerance);
    let mut since = 0i64;
    bits.push(false);
    let count = package.len();
    for n in 0..count {
        let (pulse, gap) = (i64::from(package.pulses[n]), i64::from(package.gaps[n]));
        if s.tolerance > 0 && (pulse < low || pulse > high || gap < low || gap > high) {
            if pulse as f64 > edge && pulse <= high {
                bits.push(true);
            }
            bits.new_row();
            bits.push(false);
            since = 0;
        } else if (pulse + since) as f64 > edge {
            bits.push(true);
            since = 0;
        } else {
            since += pulse;
        }
        if n + 1 == count || gap > s.reset {
            emit(bits);
            bits.clear();
            bits.push(false);
            since = 0;
        } else if (gap + since) as f64 > edge {
            bits.push(false);
            since = 0;
        } else {
            since += gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_wire::IsmModulation;

    fn us(values: &[u32]) -> Vec<u32> {
        values.iter().map(|v| v / 4).collect()
    }

    fn rows_of(
        coding: IsmCoding,
        timing: Timing,
        pulses: &[u32],
        gaps: &[u32],
    ) -> Vec<Vec<String>> {
        let (pulses, gaps) = (us(pulses), us(gaps));
        let package = Package {
            pulses: &pulses,
            gaps: &gaps,
            modulation: IsmModulation::Ook,
            snr_db: 20.0,
        };
        let mut bits = Bits::new();
        let mut seen = Vec::new();
        slice(coding, timing, &package, &mut bits, &mut |bits| {
            seen.push(
                (0..bits.rows())
                    .map(|row| {
                        (0..bits.len(row))
                            .map(|k| if bits.bit(row, k) { '1' } else { '0' })
                            .collect()
                    })
                    .collect(),
            );
        });
        seen
    }

    #[test]
    fn pwm_reads_short_pulses_as_ones_and_splits_on_long_gaps() {
        let timing = Timing::new(400, 800, 3_000).gap(1_500);
        let rows = rows_of(
            IsmCoding::Pwm,
            timing,
            &[400, 800, 400, 800, 800],
            &[800, 400, 2_000, 400, 10_000],
        );
        assert_eq!(rows, vec![vec!["101".to_owned(), "00".to_owned()]]);
    }

    #[test]
    fn ppm_reads_gap_lengths_and_ends_on_the_reset_gap() {
        let timing = Timing::new(1_000, 2_000, 5_000).gap(3_000);
        let rows = rows_of(
            IsmCoding::Ppm,
            timing,
            &[500, 500, 500, 500, 500],
            &[1_000, 2_000, 4_000, 2_000, 9_000],
        );
        assert_eq!(rows, vec![vec!["01".to_owned(), "1".to_owned()]]);
    }

    #[test]
    fn pcm_counts_bit_periods_and_pads_the_final_gap() {
        let timing = Timing::new(100, 100, 1_000);
        let rows = rows_of(IsmCoding::Pcm, timing, &[100, 200, 100], &[100, 300, 2_000]);
        assert_eq!(rows, vec![vec!["101100010000000000".to_owned()]]);
    }

    #[test]
    fn manchester_zerobit_reads_edges_at_bit_centres() {
        let timing = Timing::new(500, 0, 2_400);
        let rows = rows_of(
            IsmCoding::Manchester,
            timing,
            &[500, 1_000, 500],
            &[500, 1_000, 5_000],
        );
        assert_eq!(rows, vec![vec!["0010".to_owned()]]);
    }
}
