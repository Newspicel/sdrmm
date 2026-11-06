use num_complex::Complex;
use sdrmm_dsp::{PowerAverage, SpectrumAnalyzer};

use super::{Cadence, SpectrumFrame, Tuning, cpu_analyzer, history::SpectrumHistory};

/// Averages overlapping transforms on the calling thread, in linear power.
pub(crate) struct CpuSpectrum {
    history: SpectrumHistory,
    analyzer: SpectrumAnalyzer,
    average: PowerAverage,
    db: Vec<f32>,
    tuning: Option<Tuning>,
    last: Option<SpectrumFrame>,
}

impl CpuSpectrum {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            history: SpectrumHistory::new(size),
            analyzer: cpu_analyzer(size),
            average: PowerAverage::new(size),
            db: vec![0.0; size],
            tuning: None,
            last: None,
        }
    }

    pub(crate) fn push(
        &mut self,
        samples: &[Complex<f32>],
        index: u64,
        tuning: Tuning,
        cadence: Cadence,
        publish: &mut impl FnMut(SpectrumFrame, &[f32]),
    ) {
        if self.tuning != Some(tuning) {
            self.average.reset();
            self.tuning = Some(tuning);
            self.last = None;
        }
        let Self {
            history,
            analyzer,
            average,
            db,
            last,
            ..
        } = self;
        history.push(samples, index, cadence.hop, |window, timestamp| {
            analyzer.accumulate(window, average);
            let frame = tuning.frame(timestamp);
            *last = Some(frame);
            if average.count() >= cadence.averages.max(1) {
                average.take_db(db);
                *last = None;
                publish(frame, db);
            }
        });
    }

    pub(crate) fn restart(&mut self, publish: &mut impl FnMut(SpectrumFrame, &[f32])) {
        self.history.reset();
        self.flush(publish);
    }

    pub(crate) fn idle(
        &mut self,
        cadence: Cadence,
        publish: &mut impl FnMut(SpectrumFrame, &[f32]),
    ) {
        if self.history.settling(cadence.hop) {
            self.flush(publish);
        }
    }

    fn flush(&mut self, publish: &mut impl FnMut(SpectrumFrame, &[f32])) {
        if let Some(frame) = self.last.take()
            && self.average.count() > 0
        {
            self.average.take_db(&mut self.db);
            publish(frame, &self.db);
        }
        self.average.reset();
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_test_support::assert_no_alloc;

    use super::*;

    const SIZE: usize = 64;

    fn tuning(center_hz: f64) -> Tuning {
        Tuning {
            center_hz,
            span_hz: 2_048_000.0,
        }
    }

    fn tone(len: usize, amplitude: f32) -> Vec<Complex<f32>> {
        (0..len)
            .map(|n| Complex::from_polar(amplitude, 0.4 * n as f32))
            .collect()
    }

    fn frames(
        spectrum: &mut CpuSpectrum,
        samples: &[Complex<f32>],
        tuning: Tuning,
        cadence: Cadence,
    ) -> Vec<(SpectrumFrame, f32)> {
        let mut out = Vec::new();
        spectrum.push(samples, 0, tuning, cadence, &mut |frame, db| {
            out.push((frame, db.iter().copied().fold(f32::MIN, f32::max)));
        });
        out
    }

    #[test]
    fn publishes_once_every_wanted_transforms() {
        let mut spectrum = CpuSpectrum::new(SIZE);
        let cadence = Cadence {
            hop: SIZE,
            averages: 2,
        };
        let published = frames(&mut spectrum, &tone(SIZE * 4, 1.0), tuning(1e8), cadence);
        let stamps: Vec<_> = published.iter().map(|(frame, _)| frame.timestamp).collect();
        assert_eq!(stamps, [SIZE as u64 * 2, SIZE as u64 * 4]);
        assert!(published.iter().all(|(_, peak)| peak.abs() < 1.0));
    }

    #[test]
    fn a_retune_starts_the_average_over() {
        let mut spectrum = CpuSpectrum::new(SIZE);
        let cadence = Cadence {
            hop: SIZE,
            averages: 2,
        };
        assert!(frames(&mut spectrum, &tone(SIZE, 1.0), tuning(1e8), cadence).is_empty());
        let published = frames(&mut spectrum, &tone(SIZE * 2, 0.01), tuning(2e8), cadence);
        assert_eq!(published.len(), 1);
        assert!((published[0].1 - -40.0).abs() < 1.0, "{}", published[0].1);
    }

    #[test]
    fn a_gap_publishes_what_was_averaged_so_far() {
        let mut spectrum = CpuSpectrum::new(SIZE);
        let cadence = Cadence {
            hop: SIZE,
            averages: 16,
        };
        let mut published = Vec::new();
        let mut publish = |frame: SpectrumFrame, _: &[f32]| published.push(frame.timestamp);
        spectrum.restart(&mut publish);
        spectrum.push(&tone(SIZE, 1.0), 0, tuning(1e8), cadence, &mut publish);
        spectrum.restart(&mut publish);
        spectrum.restart(&mut publish);
        assert_eq!(published, [SIZE as u64]);
    }

    #[test]
    fn a_short_hop_overlaps_transforms() {
        let mut spectrum = CpuSpectrum::new(SIZE);
        let cadence = Cadence {
            hop: SIZE / 4,
            averages: 4,
        };
        let published = frames(&mut spectrum, &tone(SIZE * 4, 1.0), tuning(1e8), cadence);
        assert_eq!(published.len(), 4);
    }

    #[test]
    fn averaging_does_not_allocate() {
        let mut spectrum = CpuSpectrum::new(4_096);
        let input = tone(4_096 * 4, 0.5);
        let cadence = Cadence {
            hop: 1_024,
            averages: 4,
        };
        let mut publish = |_: SpectrumFrame, db: &[f32]| {
            std::hint::black_box(db);
        };
        spectrum.push(&input, 0, tuning(1e8), cadence, &mut publish);
        assert_no_alloc("spectrum averaging", || {
            spectrum.push(&input, 0, tuning(1e8), cadence, &mut publish);
        });
    }
}
