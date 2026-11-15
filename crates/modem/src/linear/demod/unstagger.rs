use num_complex::Complex;

/// Delays the in-phase rail by the transmitter's stagger so both rails sample together.
#[derive(Clone, Debug, Default)]
pub(super) struct Unstagger {
    line: Vec<f32>,
    at: usize,
}

impl Unstagger {
    pub(super) fn new(len: usize) -> Self {
        Self {
            line: vec![0.0; len],
            at: 0,
        }
    }

    pub(super) fn apply(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        out.reserve(iq.len());
        if self.line.is_empty() {
            out.extend_from_slice(iq);
            return;
        }
        let n = self.line.len();
        for &s in iq {
            let delayed = self.line[self.at];
            self.line[self.at] = s.re;
            self.at = (self.at + 1) % n;
            out.push(Complex::new(delayed, s.im));
        }
    }

    pub(super) fn reset(&mut self) {
        self.line.fill(0.0);
        self.at = 0;
    }
}
