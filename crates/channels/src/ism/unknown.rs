use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage, UNKNOWN_MODEL};

use super::{
    bits::{Bits, hex},
    pulse::{MAX_PULSES, Package},
    slicer::{Timing, slice},
};

const MIN_PULSES: usize = 24;
const MIN_BITS: usize = 12;
const MIN_REPEATS: usize = 2;
const MIN_EACH: usize = 4;
const CLUSTERS: usize = 4;
const SPREAD: f32 = 1.35;
const MIN_SHARE: usize = 10;
const PERIOD_TOLERANCE: f32 = 0.2;
const PERIODIC_SHARE: f32 = 0.8;
const MAX_MULTIPLE: f32 = 4.0;
const MULTIPLE_TOLERANCE: f32 = 0.22;
const PCM_GAP_UNITS: u32 = 16;
const RESET_US: u32 = 1_000_000;
const ID_BYTES: usize = 12;
const US_PER_SAMPLE: f32 = 4.0;

#[derive(Clone, Copy, Default)]
struct Cluster {
    sum: u64,
    count: usize,
}

impl Cluster {
    fn mean(self) -> f32 {
        self.sum as f32 / self.count.max(1) as f32
    }
}

#[derive(Default)]
struct Clusters {
    found: [Cluster; CLUSTERS],
    len: usize,
}

impl Clusters {
    fn significant(&self, total: usize) -> impl Iterator<Item = f32> + '_ {
        self.found[..self.len]
            .iter()
            .filter(move |cluster| cluster.count * MIN_SHARE >= total)
            .map(|cluster| cluster.mean())
    }

    fn means<const N: usize>(&self, total: usize) -> ([f32; N], usize) {
        let mut means = [0.0; N];
        let mut len = 0;
        for (slot, mean) in means.iter_mut().zip(self.significant(total)) {
            *slot = mean;
            len += 1;
        }
        (means, len)
    }
}

pub(super) struct Unknown {
    widths: Box<[u32; MAX_PULSES]>,
}

impl Unknown {
    pub(super) fn new() -> Self {
        Self {
            widths: Box::new([0; MAX_PULSES]),
        }
    }

    pub(super) fn analyse(&mut self, package: &Package<'_>, bits: &mut Bits) -> Option<IsmMessage> {
        if package.len() < MIN_PULSES {
            return None;
        }
        let (coding, timing) = self.estimate(package)?;
        let mut found = None;
        slice(coding, timing, package, bits, &mut |bits| {
            if found.is_none() {
                found = repeated(bits, coding, timing);
            }
        });
        found.map(|message| IsmMessage {
            modulation: package.modulation,
            snr_db: package.snr_db,
            ..message
        })
    }

    fn clusters(&mut self, values: &[u32]) -> Clusters {
        let widths = &mut self.widths[..values.len()];
        widths.copy_from_slice(values);
        widths.sort_unstable();
        let mut clusters = Clusters::default();
        for &width in widths.iter().filter(|&&width| width > 0) {
            let joins = clusters.len > 0
                && (width as f32) <= clusters.found[clusters.len - 1].mean() * SPREAD;
            if !joins {
                if clusters.len == CLUSTERS {
                    break;
                }
                clusters.len += 1;
            }
            let cluster = &mut clusters.found[clusters.len - 1];
            cluster.sum += u64::from(width);
            cluster.count += 1;
        }
        clusters
    }

    fn periodic(&mut self, package: &Package<'_>) -> bool {
        let count = package.len() - 1;
        let periods = &mut self.widths[..count];
        for (n, period) in periods.iter_mut().enumerate() {
            *period = package.pulses[n] + package.gaps[n];
        }
        steady(periods)
    }

    fn estimate(&mut self, package: &Package<'_>) -> Option<(IsmCoding, Timing)> {
        let count = package.len();
        let inner = &package.gaps[..count - 1];
        let (pulses, pulse_kinds) = self.clusters(package.pulses).means::<CLUSTERS>(count);
        let (gaps, gap_kinds) = self.clusters(inner).means::<CLUSTERS>(count);
        if pulse_kinds == 0 || gap_kinds == 0 {
            return None;
        }
        let us = |samples: f32| (samples * US_PER_SAMPLE) as u32;
        if pulse_kinds >= 2 && self.periodic(package) {
            let (short, long) = (us(pulses[0]), us(pulses[1]));
            return Some((
                IsmCoding::Pwm,
                Timing::new(short, long, RESET_US).gap(2 * (short + long)),
            ));
        }
        if pulse_kinds == 1 && gap_kinds >= 2 {
            let (short, long) = (us(gaps[0]), us(gaps[1]));
            let gap = (2 * long).saturating_sub(short).max(long + long / 4);
            return Some((IsmCoding::Ppm, Timing::new(short, long, RESET_US).gap(gap)));
        }
        let base = pulses[0].min(gaps[0]);
        let multiples = pulses[..pulse_kinds]
            .iter()
            .chain(&gaps[..gap_kinds])
            .all(|&mean| whole_multiple(mean / base));
        multiples.then(|| {
            let unit = us(base);
            (
                IsmCoding::Pcm,
                Timing::new(unit, unit, RESET_US).gap(PCM_GAP_UNITS * unit),
            )
        })
    }
}

fn whole_multiple(ratio: f32) -> bool {
    ratio <= MAX_MULTIPLE + MULTIPLE_TOLERANCE
        && (ratio - ratio.round()).abs() <= MULTIPLE_TOLERANCE
}

fn steady(periods: &mut [u32]) -> bool {
    let count = periods.len();
    let (_, &mut median, _) = periods.select_nth_unstable(count / 2);
    let median = median as f32;
    let steady = periods
        .iter()
        .filter(|&&period| (period as f32 - median).abs() <= median * PERIOD_TOLERANCE)
        .count();
    steady as f32 >= count as f32 * PERIODIC_SHARE
}

fn repeated(bits: &Bits, coding: IsmCoding, timing: Timing) -> Option<IsmMessage> {
    let row = bits.repeated_row(MIN_REPEATS, MIN_BITS)?;
    let len = bits.len(row);
    let bytes = &bits.row(row)[..len.div_ceil(8)];
    let ones = (0..len).filter(|&at| bits.bit(row, at)).count();
    if ones < MIN_EACH || len - ones < MIN_EACH {
        return None;
    }
    Some(IsmMessage {
        model: UNKNOWN_MODEL.to_owned(),
        id: hex(&bytes[..bytes.len().min(ID_BYTES)]),
        coding,
        integrity: IsmIntegrity::Repeats,
        short_us: u16::try_from(timing.short_us).ok(),
        long_us: u16::try_from(timing.long_us).ok(),
        bits: u16::try_from(len).unwrap_or(u16::MAX),
        raw: hex(bytes),
        ..IsmMessage::default()
    })
}
