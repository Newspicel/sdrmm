use super::coding::{BLOCK_BYTES, DataBlock};

const MAX_BLOCKS: usize = 9;
const MIN_MULTI_BLOCKS: usize = 4;
pub(crate) const MAX_FRAME_BYTES: usize = MAX_BLOCKS * BLOCK_BYTES;

const SINGLE: u8 = 0;
const OPEN_OR_PARITY: u8 = 1;
const EDGE: u8 = 2;
const BODY: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Opened,
    Middle,
    Closing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Completed {
    pub(crate) len: usize,
    pub(crate) start: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) lost: bool,
    pub(crate) completed: Option<Completed>,
}

pub(crate) struct Assembler {
    blocks: [[u8; BLOCK_BYTES]; MAX_BLOCKS],
    count: usize,
    hole: Option<usize>,
    stage: Stage,
    start: u64,
    idle_hole: Option<u64>,
    primed: bool,
    out: [u8; MAX_FRAME_BYTES],
}

impl Assembler {
    pub(crate) fn new() -> Self {
        Self {
            blocks: [[0; BLOCK_BYTES]; MAX_BLOCKS],
            count: 0,
            hole: None,
            stage: Stage::Idle,
            start: 0,
            idle_hole: None,
            primed: false,
            out: [0; MAX_FRAME_BYTES],
        }
    }

    pub(crate) fn bytes(&self, completed: Completed) -> &[u8] {
        &self.out[..completed.len]
    }

    pub(crate) fn clear(&mut self) {
        self.reset();
        self.primed = false;
    }

    fn reset(&mut self) {
        self.count = 0;
        self.hole = None;
        self.stage = Stage::Idle;
        self.idle_hole = None;
    }

    pub(crate) fn push(&mut self, block: Option<&DataBlock>, at: u64) -> Step {
        match block {
            None => self.missing(at),
            Some(block) => self.block(block, at),
        }
    }

    fn missing(&mut self, at: u64) -> Step {
        match self.stage {
            Stage::Idle => {
                self.idle_hole = Some(at);
                Step::default()
            }
            Stage::Closing => self.finish(false),
            _ if self.hole.is_some() || self.count == MAX_BLOCKS => {
                self.reset();
                Step::default()
            }
            _ => {
                self.hole = Some(self.count);
                self.append(&[0; BLOCK_BYTES]);
                Step::default()
            }
        }
    }

    fn block(&mut self, block: &DataBlock, at: u64) -> Step {
        let idle_hole = self.idle_hole.take();
        let primed = std::mem::replace(&mut self.primed, true);
        match (block.flag, self.stage) {
            (SINGLE, _) => {
                let lost = self.abandon();
                self.out[..BLOCK_BYTES].copy_from_slice(&block.octets);
                Step {
                    lost,
                    completed: Some(Completed {
                        len: BLOCK_BYTES,
                        start: at,
                    }),
                }
            }
            (OPEN_OR_PARITY, Stage::Closing) => {
                self.append(&block.octets);
                self.finish(true)
            }
            (OPEN_OR_PARITY, Stage::Middle) if self.hole == Some(self.count - 1) => {
                self.append(&block.octets);
                self.finish(true)
            }
            (OPEN_OR_PARITY, _) => {
                let lost = self.abandon();
                self.open(at);
                self.append(&block.octets);
                Step {
                    lost,
                    completed: None,
                }
            }
            (BODY, Stage::Opened) if self.hole.is_none() => {
                self.append(&block.octets);
                let len = self.gather(self.count);
                let start = self.start;
                self.reset();
                Step {
                    lost: false,
                    completed: Some(Completed { len, start }),
                }
            }
            (BODY, Stage::Middle) | (EDGE, Stage::Opened | Stage::Middle)
                if self.count < MAX_BLOCKS =>
            {
                if block.flag == EDGE {
                    self.stage = match self.stage {
                        Stage::Opened => Stage::Middle,
                        _ => Stage::Closing,
                    };
                }
                self.append(&block.octets);
                Step::default()
            }
            (EDGE, Stage::Idle) if idle_hole.is_some_and(|hole| hole + 1 == at) => {
                self.open(at - 1);
                self.hole = Some(0);
                self.append(&[0; BLOCK_BYTES]);
                self.append(&block.octets);
                self.stage = Stage::Middle;
                Step::default()
            }
            _ => Step {
                lost: self.abandon() || primed && idle_hole.is_none(),
                completed: None,
            },
        }
    }

    fn open(&mut self, at: u64) {
        self.count = 0;
        self.hole = None;
        self.stage = Stage::Opened;
        self.start = at;
    }

    fn append(&mut self, octets: &[u8; BLOCK_BYTES]) {
        if let Some(slot) = self.blocks.get_mut(self.count) {
            *slot = *octets;
            self.count += 1;
        }
    }

    fn abandon(&mut self) -> bool {
        let lost = self.stage != Stage::Idle && self.hole.is_none();
        self.reset();
        lost
    }

    fn finish(&mut self, with_parity: bool) -> Step {
        let data_blocks = if with_parity {
            self.count - 1
        } else {
            self.count
        };
        let start = self.start;
        let hole = self.hole;
        if self.count < MIN_MULTI_BLOCKS - usize::from(!with_parity) {
            self.reset();
            return Step {
                lost: hole.is_none(),
                completed: None,
            };
        }
        if with_parity && !self.parity_holds() {
            self.reset();
            return Step {
                lost: true,
                completed: None,
            };
        }
        if !with_parity && hole.is_some() {
            self.reset();
            return Step::default();
        }
        let len = self.gather(data_blocks);
        self.reset();
        Step {
            lost: false,
            completed: Some(Completed { len, start }),
        }
    }

    fn parity_holds(&mut self) -> bool {
        let mut parity = [0u8; BLOCK_BYTES];
        for block in &self.blocks[..self.count] {
            for (sum, octet) in parity.iter_mut().zip(block) {
                *sum ^= octet;
            }
        }
        match self.hole {
            Some(hole) => {
                self.blocks[hole] = parity;
                true
            }
            None => parity == [0; BLOCK_BYTES],
        }
    }

    fn gather(&mut self, blocks: usize) -> usize {
        for (k, block) in self.blocks[..blocks].iter().enumerate() {
            self.out[k * BLOCK_BYTES..(k + 1) * BLOCK_BYTES].copy_from_slice(block);
        }
        blocks * BLOCK_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(flag: u8, fill: u8) -> DataBlock {
        DataBlock {
            flag,
            octets: [fill; BLOCK_BYTES],
            asb: [0; 2],
        }
    }

    fn feed(assembler: &mut Assembler, blocks: &[Option<DataBlock>]) -> Vec<(Step, Vec<u8>)> {
        blocks
            .iter()
            .enumerate()
            .map(|(at, block)| {
                let step = assembler.push(block.as_ref(), at as u64);
                let bytes = step
                    .completed
                    .map(|done| assembler.bytes(done).to_vec())
                    .unwrap_or_default();
                (step, bytes)
            })
            .collect()
    }

    fn parity(fills: &[u8]) -> u8 {
        fills.iter().fold(0, |sum, fill| sum ^ fill)
    }

    #[test]
    fn single_and_bi_blocks_complete() {
        let mut assembler = Assembler::new();
        let steps = feed(
            &mut assembler,
            &[Some(block(0, 1)), Some(block(1, 2)), Some(block(3, 3))],
        );
        assert_eq!(steps[0].1, vec![1; 8]);
        assert!(steps[1].0.completed.is_none());
        assert_eq!(steps[2].1, [vec![2; 8], vec![3; 8]].concat());
        assert!(steps.iter().all(|(step, _)| !step.lost));
    }

    #[test]
    fn a_multi_block_repairs_one_lost_block_from_parity() {
        let fills = [0x11, 0x22, 0x33, 0x44];
        let mut assembler = Assembler::new();
        let steps = feed(
            &mut assembler,
            &[
                Some(block(1, fills[0])),
                Some(block(2, fills[1])),
                None,
                Some(block(2, fills[3])),
                Some(block(1, parity(&fills))),
            ],
        );
        let expected: Vec<u8> = fills.iter().flat_map(|&fill| [fill; 8]).collect();
        assert_eq!(steps[4].1, expected);
    }

    #[test]
    fn a_lost_opening_block_is_rebuilt() {
        let fills = [0x90, 0x00, 0x46];
        let mut assembler = Assembler::new();
        let steps = feed(
            &mut assembler,
            &[
                None,
                Some(block(2, fills[1])),
                Some(block(2, fills[2])),
                Some(block(1, parity(&fills))),
            ],
        );
        assert_eq!(steps[3].0.completed.map(|done| done.start), Some(0));
        assert_eq!(&steps[3].1[..8], &[0x90; 8]);
    }

    #[test]
    fn a_lost_parity_block_still_completes() {
        let mut assembler = Assembler::new();
        let steps = feed(
            &mut assembler,
            &[
                Some(block(1, 1)),
                Some(block(2, 2)),
                Some(block(2, 3)),
                None,
            ],
        );
        assert_eq!(steps[3].1.len(), 24);
    }

    #[test]
    fn broken_parity_and_orphans_are_reported_lost() {
        let mut assembler = Assembler::new();
        let steps = feed(
            &mut assembler,
            &[
                Some(block(1, 1)),
                Some(block(2, 2)),
                Some(block(2, 3)),
                Some(block(1, 9)),
                Some(block(3, 4)),
            ],
        );
        assert!(steps[3].0.lost);
        assert!(steps[3].0.completed.is_none());
        assert!(steps[4].0.lost);
    }
}
