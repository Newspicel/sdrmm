use super::{
    ConvCode, MAX_OUTPUTS,
    trellis::{Trellis, trace},
};
use crate::coding::fec::conv::{ERASURE, Soft};

const MIN_TRACEBACK_DEPTH: usize = 8;
/// Every release traces `depth` decisions to find the surviving path before it emits a single
/// bit, so releasing in batches this many times the depth spreads that fixed cost over more
/// output.
const RELEASES_PER_TRACEBACK: usize = 8;

pub struct StreamViterbiK7 {
    code: ConvCode,
    trellis: Trellis,
    decisions: Vec<u64>,
    depth: usize,
    batch: usize,
    pending: Vec<bool>,
    carry: [Soft; MAX_OUTPUTS],
    carried: usize,
}

impl StreamViterbiK7 {
    #[must_use]
    pub fn new(code: ConvCode, depth: usize) -> Self {
        let depth = depth.max(MIN_TRACEBACK_DEPTH);
        let batch = RELEASES_PER_TRACEBACK * depth;
        let mut trellis = Trellis::new();
        trellis.open();
        Self {
            code,
            trellis,
            decisions: Vec::with_capacity(batch),
            depth,
            batch,
            pending: Vec::with_capacity(batch - depth),
            carry: [ERASURE; MAX_OUTPUTS],
            carried: 0,
        }
    }

    #[must_use]
    pub const fn outputs(&self) -> usize {
        self.code.outputs()
    }

    pub fn reset(&mut self) {
        self.trellis = Trellis::new();
        self.trellis.open();
        self.decisions.clear();
        self.pending.clear();
        self.carried = 0;
    }

    fn step(&mut self, symbol: &[Soft], out: &mut Vec<bool>) {
        let word = self.trellis.step(&self.code, symbol);
        self.decisions.push(word);
        self.trellis.normalize();
        if self.decisions.len() >= self.batch {
            self.release(out);
        }
    }

    pub fn push(&mut self, coded: &[Soft], out: &mut Vec<bool>) {
        let outputs = self.code.outputs();
        let mut source = coded;
        if self.carried > 0 {
            let take = (outputs - self.carried).min(source.len());
            self.carry[self.carried..self.carried + take].copy_from_slice(&source[..take]);
            self.carried += take;
            source = &source[take..];
            if self.carried < outputs {
                return;
            }
            self.carried = 0;
            let symbol = self.carry;
            self.step(&symbol[..outputs], out);
        }
        let usable = source.len() / outputs * outputs;
        let mut offset = 0;
        while offset < usable {
            let room = self.batch - self.decisions.len();
            let take = ((usable - offset) / outputs).min(room) * outputs;
            self.trellis.push_steps(
                &self.code,
                &source[offset..offset + take],
                &mut self.decisions,
            );
            offset += take;
            if self.decisions.len() >= self.batch {
                self.release(out);
            }
        }
        self.carried = source.len() - usable;
        self.carry[..self.carried].copy_from_slice(&source[usable..]);
    }

    fn release(&mut self, out: &mut Vec<bool>) {
        let settled = self.decisions.len() - self.depth;
        let survivor = self.trellis.best_state();
        let state = trace(&self.decisions[settled..], survivor, |_| {});
        self.pending.clear();
        let pending = &mut self.pending;
        trace(&self.decisions[..settled], state, |bit| pending.push(bit));
        out.extend(self.pending.iter().rev().copied());
        self.decisions.copy_within(settled.., 0);
        self.decisions.truncate(self.depth);
    }

    pub fn flush(&mut self, out: &mut Vec<bool>) {
        if self.decisions.is_empty() {
            return;
        }
        let survivor = self.trellis.best_state();
        self.pending.clear();
        let pending = &mut self.pending;
        trace(&self.decisions, survivor, |bit| pending.push(bit));
        out.extend(self.pending.iter().rev().copied());
        self.decisions.clear();
    }
}
