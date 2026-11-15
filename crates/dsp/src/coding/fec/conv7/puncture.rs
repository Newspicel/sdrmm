use crate::coding::fec::conv::{ERASURE, Soft};

pub fn puncture(coded: &[bool], pattern: &[bool], out: &mut Vec<bool>) {
    for (index, &bit) in coded.iter().enumerate() {
        if pattern[index % pattern.len()] {
            out.push(bit);
        }
    }
}

pub fn depuncture(received: &[Soft], pattern: &[bool], out: &mut Vec<Soft>) {
    let kept = pattern.iter().filter(|&&keep| keep).count();
    if kept == 0 {
        return;
    }
    let cycles = received.len().div_ceil(kept);
    let mut source = received.iter();
    for _ in 0..cycles {
        for &keep in pattern {
            match keep.then(|| source.next()) {
                Some(Some(&value)) => out.push(value),
                Some(None) => return,
                None => out.push(ERASURE),
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Depuncturer {
    pattern: Vec<bool>,
    at: usize,
}

impl Depuncturer {
    #[must_use]
    pub fn new(pattern: &[bool]) -> Self {
        Self {
            pattern: pattern.to_vec(),
            at: 0,
        }
    }

    pub fn set_pattern(&mut self, pattern: &[bool]) {
        self.pattern.clear();
        self.pattern.extend_from_slice(pattern);
        self.at = 0;
    }

    pub fn reset(&mut self) {
        self.at = 0;
    }

    pub fn process(&mut self, received: &[Soft], out: &mut Vec<Soft>) {
        if !self.pattern.iter().any(|&keep| keep) {
            return;
        }
        for &value in received {
            while !self.pattern[self.at] {
                out.push(ERASURE);
                self.at = (self.at + 1) % self.pattern.len();
            }
            out.push(value);
            self.at = (self.at + 1) % self.pattern.len();
        }
    }
}
