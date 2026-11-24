//! Binary CPFSK at two samples a bit: phase steps between samples, a search for either sync
//! word on both sample phases, and a slicer whose threshold the sync word sets.

use num_complex::Complex;
use sdrmm_dsp::{hamming_distance, phase_diff_into};

use super::fec::{Codes, Downlink, LONG_BYTES, UPLINK_BYTES, UPLINK_DATA};

pub(crate) const SAMPLES_PER_BIT: usize = 2;
pub(crate) const SYNC_BITS: usize = 36;
const SYNC_MASK: u64 = (1 << SYNC_BITS) - 1;
pub(crate) const ADSB_SYNC: u64 = 0xE_ACDD_A4E2;
pub(crate) const UPLINK_SYNC: u64 = !ADSB_SYNC & SYNC_MASK;
/// DO-282B lets a receiver accept a sync word with up to four wrong bits.
const SYNC_ERRORS: u32 = 4;
const SYNC_SPAN: usize = SYNC_BITS * SAMPLES_PER_BIT;
const UPLINK_BITS: usize = UPLINK_BYTES * 8;
/// Every sample a frame may need past the first of its sync word, the other phase included.
const MAX_SPAN: usize = SYNC_SPAN + UPLINK_BITS * SAMPLES_PER_BIT + SAMPLES_PER_BIT;

/// What one sync word led to.
#[derive(Debug, PartialEq)]
pub(crate) enum Heard<'a> {
    Downlink {
        frame: Downlink,
        level: f32,
        at: u64,
    },
    Uplink {
        data: &'a [u8; UPLINK_DATA],
        corrected: u32,
        level: f32,
        at: u64,
    },
}

pub(crate) trait Sink {
    fn heard(&mut self, heard: Heard<'_>);
}

pub(crate) struct Receiver {
    codes: Codes,
    previous: Complex<f32>,
    fresh: Vec<f32>,
    steps: Vec<f32>,
    power: Vec<f32>,
    origin: u64,
    cursor: usize,
    registers: [u64; SAMPLES_PER_BIT],
    rejected: u32,
    failed: Option<u64>,
    adsb: [[u8; LONG_BYTES]; SAMPLES_PER_BIT],
    adsb_reliability: [f32; LONG_BYTES],
    raw: [u8; UPLINK_BYTES],
    reliability: [f32; UPLINK_BYTES],
    blocks: [u8; UPLINK_BYTES],
    uplinks: [[u8; UPLINK_DATA]; SAMPLES_PER_BIT],
    tapping: bool,
    soft: Vec<f32>,
    centre: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    Adsb,
    Uplink,
}

impl Link {
    const fn bits(self) -> usize {
        match self {
            Self::Adsb => LONG_BYTES * 8,
            Self::Uplink => UPLINK_BITS,
        }
    }

    /// Samples a frame needs past its sync word's last bit, the other phase included.
    const fn span(self) -> usize {
        (self.bits() + 1) * SAMPLES_PER_BIT
    }
}

impl Receiver {
    pub(crate) fn new() -> Self {
        Self {
            codes: Codes::new(),
            previous: Complex::default(),
            fresh: Vec::new(),
            steps: Vec::with_capacity(4 * MAX_SPAN),
            power: Vec::with_capacity(4 * MAX_SPAN),
            origin: 0,
            cursor: 0,
            registers: [0; SAMPLES_PER_BIT],
            rejected: 0,
            failed: None,
            adsb: [[0; LONG_BYTES]; SAMPLES_PER_BIT],
            adsb_reliability: [0.0; LONG_BYTES],
            raw: [0; UPLINK_BYTES],
            reliability: [0.0; UPLINK_BYTES],
            blocks: [0; UPLINK_BYTES],
            uplinks: [[0; UPLINK_DATA]; SAMPLES_PER_BIT],
            tapping: false,
            soft: Vec::with_capacity(UPLINK_BITS),
            centre: 0.0,
        }
    }

    /// Keeps each heard frame's phase steps for [`Self::soft`].
    pub(crate) fn set_tapping(&mut self, tapping: bool) {
        self.tapping = tapping;
    }

    /// The phase step of every bit heard in the last [`Self::process`], the slicer's
    /// threshold taken off so the sign is the bit.
    pub(crate) fn soft(&self) -> &[f32] {
        &self.soft
    }

    /// The last heard frame's slicing threshold: the carrier's phase step per sample.
    pub(crate) fn centre(&self) -> f32 {
        self.centre
    }

    /// Sync words found whose frame the code could not repair.
    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    pub(crate) fn reset(&mut self) {
        self.previous = Complex::default();
        self.steps.clear();
        self.power.clear();
        self.cursor = 0;
        self.registers = [0; SAMPLES_PER_BIT];
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.soft.clear();
        phase_diff_into(iq, &mut self.previous, 1.0, &mut self.fresh);
        self.steps.extend_from_slice(&self.fresh);
        self.power.extend(iq.iter().map(Complex::norm_sqr));
        self.scan(sink);
        self.trim();
    }

    fn scan(&mut self, sink: &mut impl Sink) {
        let len = self.steps.len();
        while self.cursor + SAMPLES_PER_BIT + Link::Adsb.span() <= len {
            let at = self.cursor;
            let phase = at % SAMPLES_PER_BIT;
            let word = (self.registers[phase] << 1 | u64::from(self.steps[at] > 0.0)) & SYNC_MASK;
            let link = if hamming_distance(word, ADSB_SYNC) <= SYNC_ERRORS {
                Some(Link::Adsb)
            } else if hamming_distance(word, UPLINK_SYNC) <= SYNC_ERRORS {
                Some(Link::Uplink)
            } else {
                None
            };
            if link.is_some_and(|link| at + SAMPLES_PER_BIT + link.span() > len) {
                return;
            }
            self.registers[phase] = word;
            self.cursor += 1;
            let Some(link) = link else {
                continue;
            };
            let Some(start) = (at + SAMPLES_PER_BIT).checked_sub(SYNC_SPAN) else {
                continue;
            };
            let absolute = self.origin + start as u64;
            if self
                .failed
                .is_some_and(|failed| absolute < failed + SAMPLES_PER_BIT as u64)
            {
                continue;
            }
            if let Some(span) = self.frame(link, start, sink) {
                self.cursor = start + span;
                self.registers = [0; SAMPLES_PER_BIT];
            } else {
                self.failed = Some(absolute);
                self.rejected = self.rejected.saturating_add(1);
            }
        }
    }

    /// Slices the frame whose sync word starts at `start` on this sample phase and the next,
    /// keeps the one that needed fewer repairs, and returns the samples it spans.
    fn frame(&mut self, link: Link, start: usize, sink: &mut impl Sink) -> Option<usize> {
        let span = SYNC_SPAN + link.bits() * SAMPLES_PER_BIT;
        let level = mean(&self.power[start..start + span]);
        let at = self.origin + start as u64;
        match link {
            Link::Adsb => {
                let (frame, offset) = self.best_downlink(start)?;
                self.tap(start + offset, ADSB_SYNC, link);
                sink.heard(Heard::Downlink {
                    frame,
                    level,
                    at: at + offset as u64,
                });
            }
            Link::Uplink => {
                let (corrected, offset) = self.best_uplink(start)?;
                self.tap(start + offset, UPLINK_SYNC, link);
                sink.heard(Heard::Uplink {
                    data: &self.uplinks[offset],
                    corrected,
                    level,
                    at: at + offset as u64,
                });
            }
        }
        Some(span)
    }

    fn best_downlink(&mut self, start: usize) -> Option<(Downlink, usize)> {
        let mut best: Option<(Downlink, usize)> = None;
        for offset in 0..SAMPLES_PER_BIT {
            let Some(centre) = centre(&self.steps[start + offset..], ADSB_SYNC) else {
                continue;
            };
            let frame = &mut self.adsb[offset];
            let steps = &self.steps[start + offset + SYNC_SPAN..];
            slice(steps, centre, frame, &mut self.adsb_reliability);
            if let Some(decoded) = self.codes.downlink(frame, &self.adsb_reliability)
                && best.is_none_or(|(kept, _)| decoded.corrected < kept.corrected)
            {
                best = Some((decoded, offset));
            }
        }
        best
    }

    fn best_uplink(&mut self, start: usize) -> Option<(u32, usize)> {
        let mut best: Option<(u32, usize)> = None;
        for offset in 0..SAMPLES_PER_BIT {
            let Some(centre) = centre(&self.steps[start + offset..], UPLINK_SYNC) else {
                continue;
            };
            slice(
                &self.steps[start + offset + SYNC_SPAN..],
                centre,
                &mut self.raw,
                &mut self.reliability,
            );
            if let Some(corrected) = self.codes.uplink(
                &self.raw,
                &self.reliability,
                &mut self.blocks,
                &mut self.uplinks[offset],
            ) && best.is_none_or(|(kept, _)| corrected < kept)
            {
                best = Some((corrected, offset));
            }
        }
        best
    }

    fn tap(&mut self, start: usize, word: u64, link: Link) {
        if !self.tapping {
            return;
        }
        let Some(centre) = centre(&self.steps[start..], word) else {
            return;
        };
        let bits = &self.steps[start + SYNC_SPAN..];
        self.soft.extend(
            bits.iter()
                .step_by(SAMPLES_PER_BIT)
                .take(link.bits())
                .map(|step| step - centre),
        );
        self.centre = centre;
    }

    fn trim(&mut self) {
        let keep_from = self.cursor.saturating_sub(SYNC_SPAN);
        if keep_from == 0 {
            return;
        }
        self.steps.drain(..keep_from);
        self.power.drain(..keep_from);
        self.cursor -= keep_from;
        self.origin += keep_from as u64;
    }
}

/// The slicing threshold halfway between the mean steps of the sync word's ones and zeros,
/// so a frequency offset moves both and cancels; `None` when too many sync bits then disagree.
fn centre(steps: &[f32], word: u64) -> Option<f32> {
    let mut sums = [0.0f32; 2];
    let mut counts = [0u32; 2];
    for bit in 0..SYNC_BITS {
        let one = usize::from(word >> (SYNC_BITS - 1 - bit) & 1 == 1);
        sums[one] += steps[bit * SAMPLES_PER_BIT];
        counts[one] += 1;
    }
    let centre = f32::midpoint(sums[0] / counts[0] as f32, sums[1] / counts[1] as f32);
    let errors = (0..SYNC_BITS)
        .filter(|&bit| {
            let one = word >> (SYNC_BITS - 1 - bit) & 1 == 1;
            (steps[bit * SAMPLES_PER_BIT] > centre) != one
        })
        .count();
    (errors as u32 <= SYNC_ERRORS).then_some(centre)
}

/// Each byte's bits, first sent most significant, and its reliability: the distance of its
/// least sure bit from the threshold.
fn slice(steps: &[f32], centre: f32, out: &mut [u8], reliability: &mut [f32]) {
    for (index, (byte, sure)) in out.iter_mut().zip(reliability.iter_mut()).enumerate() {
        let bits = steps[index * 8 * SAMPLES_PER_BIT..]
            .iter()
            .step_by(SAMPLES_PER_BIT)
            .take(8);
        (*byte, *sure) = bits.fold((0u8, f32::INFINITY), |(acc, sure), &step| {
            (
                acc << 1 | u8::from(step > centre),
                sure.min((step - centre).abs()),
            )
        });
    }
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uplink_sync_word_is_the_downlink_one_inverted() {
        assert_eq!(UPLINK_SYNC, 0x1_5322_5B1D);
        assert_eq!(hamming_distance(ADSB_SYNC, UPLINK_SYNC), 36);
    }

    #[test]
    fn a_byte_is_as_reliable_as_its_least_sure_bit() {
        let steps: Vec<f32> = [0.9, -0.8, 0.05, 1.0, -1.0, 0.7, -0.6, 0.8, -0.3, 0.4]
            .iter()
            .chain(&[0.5; 6])
            .flat_map(|&step| [step + 0.1; SAMPLES_PER_BIT])
            .collect();
        let (mut bytes, mut reliability) = ([0u8; 2], [0.0f32; 2]);
        slice(&steps, 0.1, &mut bytes, &mut reliability);
        assert_eq!(bytes, [0b1011_0101, 0b0111_1111]);
        assert!((reliability[0] - 0.05).abs() < 1e-6, "{reliability:?}");
        assert!((reliability[1] - 0.3).abs() < 1e-6, "{reliability:?}");
    }

    #[test]
    fn the_centre_follows_a_frequency_offset() {
        let offset = 0.4;
        let steps: Vec<f32> = (0..SYNC_BITS)
            .flat_map(|bit| {
                let step = if ADSB_SYNC >> (SYNC_BITS - 1 - bit) & 1 == 1 {
                    1.0
                } else {
                    -1.0
                };
                [step + offset; 2]
            })
            .collect();
        let centre = centre(&steps, ADSB_SYNC).expect("sync");
        assert!((centre - offset).abs() < 1e-6, "{centre}");
        assert_eq!(super::centre(&steps, UPLINK_SYNC), None);
    }
}
