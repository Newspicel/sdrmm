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
    offsets: Vec<usize>,
    at: usize,
}

impl Depuncturer {
    #[must_use]
    pub fn new(pattern: &[bool]) -> Self {
        Self {
            offsets: kept_offsets(pattern),
            pattern: pattern.to_vec(),
            at: 0,
        }
    }

    pub fn set_pattern(&mut self, pattern: &[bool]) {
        self.pattern.clear();
        self.pattern.extend_from_slice(pattern);
        self.offsets = kept_offsets(pattern);
        self.at = 0;
    }

    pub fn reset(&mut self) {
        self.at = 0;
    }

    pub fn process(&mut self, received: &[Soft], out: &mut Vec<Soft>) {
        let kept = self.offsets.len();
        if kept == 0 || received.is_empty() {
            return;
        }
        let len = self.pattern.len();
        if kept == len {
            out.extend_from_slice(received);
            self.at = (self.at + received.len()) % len;
            return;
        }
        out.reserve((received.len() / kept + 2) * len);
        let mut rest = received;
        if self.at != 0 {
            while let Some((&value, tail)) = rest.split_first() {
                while !self.pattern[self.at] {
                    out.push(ERASURE);
                    self.at += 1;
                    if self.at == len {
                        self.at = 0;
                    }
                }
                out.push(value);
                self.at += 1;
                rest = tail;
                if self.at == len {
                    self.at = 0;
                    break;
                }
            }
        }
        if rest.len() > kept {
            let cycles = (rest.len() - 1) / kept;
            let body_in = cycles * kept;
            let base = out.len();
            out.resize(base + cycles * len, ERASURE);
            for (chunk, dst) in rest[..body_in]
                .chunks_exact(kept)
                .zip(out[base..].chunks_exact_mut(len))
            {
                for (&value, &offset) in chunk.iter().zip(&self.offsets) {
                    dst[offset] = value;
                }
            }
            rest = &rest[body_in..];
        }
        for &value in rest {
            while !self.pattern[self.at] {
                out.push(ERASURE);
                self.at += 1;
                if self.at == len {
                    self.at = 0;
                }
            }
            out.push(value);
            self.at += 1;
            if self.at == len {
                self.at = 0;
            }
        }
    }
}

fn kept_offsets(pattern: &[bool]) -> Vec<usize> {
    pattern
        .iter()
        .enumerate()
        .filter_map(|(index, &keep)| keep.then_some(index))
        .collect()
}
