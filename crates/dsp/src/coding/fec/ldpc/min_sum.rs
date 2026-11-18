use super::ParityCheck;

/// The order messages are passed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schedule {
    /// Each check updates the totals in turn, so later checks see earlier ones' work.
    Layered,
    /// Every check reads the same variable messages, then every variable is updated.
    Flooding,
}

/// Normalized min-sum decoding of any [`ParityCheck`].
///
/// Log-likelihood ratios are positive for a 0 bit. Each iteration is driven by the caller
/// through [`MinSum::iterate`], so a protocol can stop early by its own rule, or by
/// [`MinSum::decode`] when converging is the only rule.
pub struct MinSum {
    code: ParityCheck,
    schedule: Schedule,
    scale: f32,
    totals: Vec<f32>,
    messages: Vec<f32>,
    incoming: Vec<f32>,
    hard: Vec<bool>,
}

impl MinSum {
    #[must_use]
    pub fn new(code: ParityCheck, schedule: Schedule, scale: f32) -> Self {
        let edges = code.edges();
        let variables = code.variables();
        Self {
            code,
            schedule,
            scale,
            totals: vec![0.0; variables],
            messages: vec![0.0; edges],
            incoming: vec![0.0; edges],
            hard: vec![false; variables],
        }
    }

    #[must_use]
    pub const fn code(&self) -> &ParityCheck {
        &self.code
    }

    /// Starts a new word. Returns the number of checks its hard decisions leave unsatisfied.
    pub fn load(&mut self, llrs: &[f32]) -> usize {
        self.messages.fill(0.0);
        match self.schedule {
            Schedule::Layered => {
                for (total, &llr) in self.totals.iter_mut().zip(llrs) {
                    *total = llr;
                }
            }
            Schedule::Flooding => self.update_variables(llrs),
        }
        self.harden()
    }

    /// Runs one iteration on `llrs`, the word given to [`MinSum::load`]. Returns the number
    /// of checks the new hard decisions leave unsatisfied.
    pub fn iterate(&mut self, llrs: &[f32]) -> usize {
        match self.schedule {
            Schedule::Layered => {
                for check in 0..self.code.checks() {
                    self.update_layer(check);
                }
            }
            Schedule::Flooding => {
                for check in 0..self.code.checks() {
                    self.update_check(check);
                }
                self.update_variables(llrs);
            }
        }
        self.harden()
    }

    /// Iterates until every check holds, at most `limit` times. Returns the iterations taken.
    pub fn decode(&mut self, llrs: &[f32], limit: usize) -> Option<usize> {
        if self.load(llrs) == 0 {
            return Some(0);
        }
        (1..=limit).find(|_| self.iterate(llrs) == 0)
    }

    /// The hard decision per variable after the last iteration.
    #[must_use]
    pub fn hard(&self) -> &[bool] {
        &self.hard
    }

    /// The posterior log-likelihood ratio per variable.
    #[must_use]
    pub fn totals(&self) -> &[f32] {
        &self.totals
    }

    fn harden(&mut self) -> usize {
        for (bit, &total) in self.hard.iter_mut().zip(&self.totals) {
            *bit = total < 0.0;
        }
        self.code.unsatisfied(&self.hard)
    }

    fn update_layer(&mut self, check: usize) {
        let edges = self.code.check_edges(check);
        let mut pair = MinPair::default();
        for (edge, &variable) in edges.clone().zip(self.code.check(check)) {
            let value = self.totals[variable] - self.messages[edge];
            self.incoming[edge] = value;
            pair.absorb(edge, value);
        }
        for (edge, &variable) in edges.zip(self.code.check(check)) {
            let message = pair.outgoing(edge, self.incoming[edge], self.scale);
            self.messages[edge] = message;
            self.totals[variable] = self.incoming[edge] + message;
        }
    }

    fn update_check(&mut self, check: usize) {
        let edges = self.code.check_edges(check);
        let mut pair = MinPair::default();
        for edge in edges.clone() {
            pair.absorb(edge, self.incoming[edge]);
        }
        for edge in edges {
            self.messages[edge] = pair.outgoing(edge, self.incoming[edge], self.scale);
        }
    }

    fn update_variables(&mut self, llrs: &[f32]) {
        for (variable, &llr) in llrs.iter().enumerate() {
            let edges = self.code.variable_edges(variable);
            let total = llr + edges.iter().map(|&edge| self.messages[edge]).sum::<f32>();
            self.totals[variable] = total;
            for &edge in edges {
                self.incoming[edge] = total - self.messages[edge];
            }
        }
    }
}

/// The two smallest magnitudes and the sign parity of one check's incoming messages.
#[derive(Clone, Copy, Debug)]
pub(super) struct MinPair {
    first: f32,
    second: f32,
    weakest: usize,
    negative: bool,
}

impl Default for MinPair {
    fn default() -> Self {
        Self {
            first: f32::MAX,
            second: f32::MAX,
            weakest: usize::MAX,
            negative: false,
        }
    }
}

impl MinPair {
    pub(super) fn absorb(&mut self, edge: usize, value: f32) {
        self.negative ^= value < 0.0;
        let magnitude = value.abs();
        if magnitude < self.first {
            self.second = self.first;
            self.first = magnitude;
            self.weakest = edge;
        } else if magnitude < self.second {
            self.second = magnitude;
        }
    }

    /// The message back along `edge`: the smallest other magnitude, scaled, with the sign
    /// that makes the check's parity even.
    pub(super) fn outgoing(&self, edge: usize, value: f32, scale: f32) -> f32 {
        let magnitude = scale
            * if edge == self.weakest {
                self.second
            } else {
                self.first
            };
        if self.negative ^ (value < 0.0) {
            -magnitude
        } else {
            magnitude
        }
    }
}
