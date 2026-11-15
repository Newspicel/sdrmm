use super::{
    ConvCode,
    trellis::{Trellis, trace},
};
use crate::coding::fec::conv::Soft;

pub struct ViterbiK7 {
    code: ConvCode,
    trellis: Trellis,
    decisions: Vec<u64>,
}

impl ViterbiK7 {
    #[must_use]
    pub fn new(code: ConvCode) -> Self {
        Self {
            code,
            trellis: Trellis::new(),
            decisions: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_capacity(code: ConvCode, steps: usize) -> Self {
        Self {
            decisions: Vec::with_capacity(steps),
            ..Self::new(code)
        }
    }

    #[must_use]
    pub const fn outputs(&self) -> usize {
        self.code.outputs()
    }

    pub fn decode(&mut self, coded: &[Soft], out: &mut Vec<bool>) -> i32 {
        self.run(coded, out, None)
    }

    pub fn decode_tailed(&mut self, coded: &[Soft], out: &mut Vec<bool>) -> i32 {
        self.run(coded, out, Some(0))
    }

    fn run(&mut self, coded: &[Soft], out: &mut Vec<bool>, end: Option<usize>) -> i32 {
        let outputs = self.code.outputs();
        let steps = coded.len() / outputs;
        self.decisions.clear();
        self.decisions.reserve(steps);
        self.trellis.restart();
        for symbol in coded[..steps * outputs].chunks_exact(outputs) {
            let word = self.trellis.step(&self.code, symbol);
            self.decisions.push(word);
            self.trellis.normalize();
        }
        let state = end.unwrap_or_else(|| self.trellis.best_state());
        let metric = self.trellis.metrics()[state];
        let start = out.len();
        trace(&self.decisions, state, |bit| out.push(bit));
        out[start..].reverse();
        metric
    }
}
