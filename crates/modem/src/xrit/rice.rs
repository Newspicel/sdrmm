//! Lossless Rice decompression of one image line (CCSDS 121.0-B, the SZIP subset NOAA uses):
//! per block a split-sample, second-extension, zero-block or raw option, then the inverse of the
//! unit-delay prediction mapping.

/// Sample bytes in most-significant-bit order; with one-byte samples it changes nothing.
pub const MSB: u16 = 16;
/// Nearest-neighbour preprocessing: each line starts with a raw reference sample.
pub const NN: u16 = 32;

const ID_LEN: u32 = 3;
const RAW_ID: u32 = (1 << ID_LEN) - 1;
const REST_OF_SEGMENT: u32 = 5;
const SEGMENT_BLOCKS: usize = 64;
const MAX_SECOND_EXTENSION: u32 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiceError {
    Unsupported,
    Truncated,
    Invalid,
}

impl std::fmt::Display for RiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "unsupported Rice parameters",
            Self::Truncated => "Rice line ends early",
            Self::Invalid => "Rice line is malformed",
        })
    }
}

impl std::error::Error for RiceError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiceParams {
    pub flags: u16,
    pub bits_per_pixel: u8,
    pub pixels_per_block: u8,
    pub columns: usize,
}

impl RiceParams {
    fn check(self) -> Result<Self, RiceError> {
        let block_ok = matches!(self.pixels_per_block, 8 | 16 | 32 | 64);
        let bits_ok = (1..=8).contains(&self.bits_per_pixel);
        if block_ok && bits_ok && self.columns > 0 {
            Ok(self)
        } else {
            Err(RiceError::Unsupported)
        }
    }

    #[must_use]
    pub fn block(self) -> usize {
        usize::from(self.pixels_per_block)
    }

    /// The line rounded up to whole blocks, as the coder sees it.
    #[must_use]
    pub fn padded(self) -> usize {
        self.columns.div_ceil(self.block()) * self.block()
    }

    #[must_use]
    pub fn preprocessed(self) -> bool {
        self.flags & NN != 0
    }

    fn max_sample(self) -> u32 {
        (1 << self.bits_per_pixel) - 1
    }
}

struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn read(&mut self, count: u32) -> Result<u32, RiceError> {
        let mut value = 0;
        for _ in 0..count {
            let byte = self.bytes.get(self.at / 8).ok_or(RiceError::Truncated)?;
            value = value << 1 | u32::from(byte >> (7 - self.at % 8) & 1);
            self.at += 1;
        }
        Ok(value)
    }

    fn fundamental(&mut self) -> Result<u32, RiceError> {
        let mut zeros = 0;
        while self.read(1)? == 0 {
            zeros += 1;
            if zeros > 1 << 16 {
                return Err(RiceError::Invalid);
            }
        }
        Ok(zeros)
    }
}

/// Decodes the lines of one image, reusing its scratch line.
pub struct RiceLine {
    params: RiceParams,
    samples: Vec<u32>,
}

impl RiceLine {
    pub fn new(params: RiceParams) -> Result<Self, RiceError> {
        let params = params.check()?;
        Ok(Self {
            params,
            samples: vec![0; params.padded()],
        })
    }

    #[must_use]
    pub fn params(&self) -> RiceParams {
        self.params
    }

    /// Appends the line `input` codes, `columns` samples, to `out`.
    pub fn decode(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), RiceError> {
        let mut bits = Bits {
            bytes: input,
            at: 0,
        };
        let block = self.params.block();
        let blocks = self.samples.len() / block;
        let mut index = 0;
        while index < blocks {
            index += self.block(&mut bits, index, blocks)?;
        }
        if self.params.preprocessed() {
            self.unpredict();
        }
        out.extend(
            self.samples[..self.params.columns]
                .iter()
                .map(|&sample| sample as u8),
        );
        Ok(())
    }

    fn block(
        &mut self,
        bits: &mut Bits<'_>,
        index: usize,
        blocks: usize,
    ) -> Result<usize, RiceError> {
        let size = self.params.block();
        let reference = usize::from(index == 0 && self.params.preprocessed());
        let start = index * size;
        let id = bits.read(ID_LEN)?;
        if id == RAW_ID {
            for slot in &mut self.samples[start..start + size] {
                *slot = bits.read(u32::from(self.params.bits_per_pixel))?;
            }
            return Ok(1);
        }
        if id == 0 {
            let second_extension = bits.read(1)? == 1;
            self.read_reference(bits, start, reference)?;
            if second_extension {
                self.second_extension(bits, start + reference, start + size, reference)?;
                return Ok(1);
            }
            return self.zero_blocks(bits, index, blocks, reference);
        }
        self.read_reference(bits, start, reference)?;
        self.split(bits, start + reference, start + size, id - 1)?;
        Ok(1)
    }

    fn read_reference(
        &mut self,
        bits: &mut Bits<'_>,
        start: usize,
        reference: usize,
    ) -> Result<(), RiceError> {
        if reference == 1 {
            self.samples[start] = bits.read(u32::from(self.params.bits_per_pixel))?;
        }
        Ok(())
    }

    fn split(
        &mut self,
        bits: &mut Bits<'_>,
        from: usize,
        to: usize,
        k: u32,
    ) -> Result<(), RiceError> {
        for slot in &mut self.samples[from..to] {
            *slot = bits.fundamental()? << k;
        }
        for slot in &mut self.samples[from..to] {
            *slot |= bits.read(k)?;
        }
        Ok(())
    }

    fn second_extension(
        &mut self,
        bits: &mut Bits<'_>,
        from: usize,
        to: usize,
        reference: usize,
    ) -> Result<(), RiceError> {
        let mut at = from;
        let mut odd = reference == 1;
        while at < to {
            let m = bits.fundamental()?;
            if m > MAX_SECOND_EXTENSION {
                return Err(RiceError::Invalid);
            }
            let (beta, base) = triangle(m);
            let second = m - base;
            if !odd {
                self.samples[at] = beta - second;
                at += 1;
            }
            if at < to {
                self.samples[at] = second;
                at += 1;
            }
            odd = false;
        }
        Ok(())
    }

    fn zero_blocks(
        &mut self,
        bits: &mut Bits<'_>,
        index: usize,
        blocks: usize,
        reference: usize,
    ) -> Result<usize, RiceError> {
        let fs = bits.fundamental()? + 1;
        let count = match fs {
            REST_OF_SEGMENT => (blocks - index).min(SEGMENT_BLOCKS - index % SEGMENT_BLOCKS),
            more if more > REST_OF_SEGMENT => (more - 1) as usize,
            fewer => fewer as usize,
        };
        if index + count > blocks {
            return Err(RiceError::Invalid);
        }
        let size = self.params.block();
        self.samples[index * size + reference..(index + count) * size].fill(0);
        Ok(count)
    }

    fn unpredict(&mut self) {
        let max = self.params.max_sample();
        let middle = max / 2 + 1;
        let Some(&first) = self.samples.first() else {
            return;
        };
        let mut last = first;
        for slot in &mut self.samples[1..] {
            last = unmap(*slot, last, max, middle);
            *slot = last;
        }
    }
}

/// The m-th second-extension code word's pair sum and the first word of its run.
fn triangle(m: u32) -> (u32, u32) {
    let mut beta = 0;
    let mut base = 0;
    while base + beta < m {
        beta += 1;
        base += beta;
    }
    (beta, base)
}

fn unmap(mapped: u32, last: u32, max: u32, middle: u32) -> u32 {
    let half = (mapped >> 1) + (mapped & 1);
    let mask = if last & middle != 0 { max } else { 0 };
    if half <= (mask ^ last) {
        if mapped & 1 == 0 {
            last + (mapped >> 1)
        } else {
            last - half
        }
    } else {
        (mask ^ mapped) & max
    }
}
