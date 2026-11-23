use num_complex::Complex;

use super::{
    Coding, Constellation, DecodeError, Frame, Rate,
    bicm::{Decoded, Decoder},
    interleave,
    signalling::{Plp, Post, Pre},
    transport::Transport,
    transport_clock::Sink,
};
use sdrmm_modem::dvb::PACKET;

const TIME_CAPACITY: usize = 1 << 19;
const TAP_CELLS: usize = 4096;
const PARALLEL_BLOCKS: usize = 16;
const MAX_MESSAGE_BITS: usize = 54_000;

mod fec;
use fec::FecLane;

#[derive(Clone, Copy, Debug, Default)]
pub struct Report {
    pub blocks: u32,
    pub packets: u32,
    pub errors: u32,
    pub corrected_bits: u32,
    pub last_error: Option<DecodeError>,
}

pub struct Scheduler {
    decoders: Vec<(Frame, Rate, Decoder)>,
    selected: Option<Plp>,
    next_frame: Option<usize>,
    buffered: Vec<Complex<f32>>,
    deinterleaved: Vec<Complex<f32>>,
    buffered_gains: Vec<f32>,
    deinterleaved_gains: Vec<f32>,
    word: Vec<bool>,
    fec_workers: Option<[FecLane; 2]>,
    transport: Transport,
    slices: usize,
    interval: usize,
    slice_length: usize,
    received: usize,
    time_block: usize,
    decoder: usize,
    active: bool,
    report: Report,
    generation: u64,
    tap: Vec<Complex<f32>>,
    tapping: bool,
    tapped: Option<Constellation>,
}

impl Scheduler {
    pub fn new(parallel: bool) -> Result<Self, DecodeError> {
        let mut decoders = Vec::with_capacity(14);
        for frame in [Frame::Short, Frame::Normal] {
            for rate in [
                Rate::R1_2,
                Rate::R3_5,
                Rate::R2_3,
                Rate::R3_4,
                Rate::R4_5,
                Rate::R5_6,
                Rate::R1_3,
                Rate::R2_5,
            ] {
                let lite = matches!(rate, Rate::R1_3 | Rate::R2_5);
                if lite && frame != Frame::Short {
                    continue;
                }
                let coding = Coding {
                    frame,
                    rate,
                    constellation: Constellation::Qpsk,
                    rotated: false,
                    lite,
                };
                decoders.push((frame, rate, Decoder::new(coding)?));
            }
        }
        Ok(Self {
            decoders,
            selected: None,
            next_frame: None,
            buffered: Vec::with_capacity(TIME_CAPACITY),
            deinterleaved: vec![Complex::default(); TIME_CAPACITY],
            buffered_gains: Vec::with_capacity(TIME_CAPACITY),
            deinterleaved_gains: vec![0.0; TIME_CAPACITY],
            word: vec![false; MAX_MESSAGE_BITS],
            fec_workers: if parallel
                && std::thread::available_parallelism().is_ok_and(|cpus| cpus.get() >= 4)
            {
                FecLane::new()
                    .zip(FecLane::new())
                    .map(|(first, second)| [first, second])
            } else {
                None
            },
            transport: Transport::default(),
            slices: 0,
            interval: 0,
            slice_length: 0,
            received: 0,
            time_block: 0,
            decoder: 0,
            active: false,
            report: Report::default(),
            generation: 0,
            tap: Vec::with_capacity(TAP_CELLS),
            tapping: false,
            tapped: None,
        })
    }

    pub fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.selected = None;
        self.next_frame = None;
        self.buffered.clear();
        self.buffered_gains.clear();
        self.transport.reset();
        self.active = false;
    }

    /// Asks the next FEC block to keep its first cells for the scope.
    pub fn request_tap(&mut self) {
        self.tapping = true;
        self.tapped = None;
        self.tap.clear();
    }

    /// The kept cells and the constellation they were sent in.
    pub fn tapped(&self) -> Option<(&[Complex<f32>], Constellation)> {
        self.tapped
            .map(|constellation| (self.tap.as_slice(), constellation))
    }

    pub fn release_tap(&mut self) {
        self.tapped = None;
        self.tap.clear();
    }

    pub(super) const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn selected(&self) -> Option<Plp> {
        self.selected
    }
    pub const fn report(&self) -> Report {
        self.report
    }

    pub fn begin(&mut self, pre: Pre, post: &Post, id: Option<u8>) -> Result<(), DecodeError> {
        self.active = false;
        if pre.rf_count != 1 {
            return Err(DecodeError::Stream);
        }
        let plp = post.select(id).ok_or(DecodeError::Plp)?;
        self.begin_plp(pre, post, plp, 0)
    }

    pub(super) fn begin_plp(
        &mut self,
        pre: Pre,
        post: &Post,
        plp: Plp,
        origin: u64,
    ) -> Result<(), DecodeError> {
        self.active = false;
        if post.frame < plp.first_frame
            || !(post.frame - plp.first_frame).is_multiple_of(plp.frame_interval)
        {
            return Ok(());
        }
        let pi = plp.interleaving_frames();
        let position = (post.frame - plp.first_frame) / plp.frame_interval % pi;
        let changed = self.selected.is_none_or(|old| {
            old.id != plp.id
                || old.coding != plp.coding
                || old.time_length != plp.time_length
                || old.time_across_frames != plp.time_across_frames
                || old.frame_interval != plp.frame_interval
        });
        let discontinuity = self.next_frame.is_some_and(|frame| frame != post.frame);
        if changed || discontinuity {
            self.reset();
        }
        if position != 0 && self.selected.is_none_or(|old| old.blocks != plp.blocks) {
            return Err(DecodeError::Discontinuity);
        }
        if position == 0 {
            self.transport.set_origin(origin);
            if !self.buffered.is_empty() {
                return Err(DecodeError::Discontinuity);
            }
            self.time_block =
                if plp.time_length > 0 && !plp.time_across_frames && plp.blocks < plp.time_length {
                    plp.time_length - plp.blocks
                } else {
                    0
                };
        }
        self.decoder = self
            .decoders
            .iter()
            .position(|(frame, rate, _)| *frame == plp.coding.frame && *rate == plp.coding.rate)
            .ok_or(DecodeError::Parameters)?;
        self.decoders[self.decoder].2.configure(plp.coding)?;
        self.slices = if plp.kind == 2 { post.subslices } else { 1 };
        self.interval = if plp.kind == 2 {
            post.subslice_interval
        } else {
            0
        };
        let total = plp.blocks * plp.coding.cells();
        if !total.is_multiple_of(pi * self.slices) {
            return Err(DecodeError::Signalling);
        }
        self.slice_length = total / pi / self.slices;
        if self.slices > 1 && self.interval < self.slice_length {
            return Err(DecodeError::Signalling);
        }
        let blocks = if plp.time_across_frames {
            plp.blocks
        } else {
            plp.blocks.div_ceil(plp.time_length.max(1))
        };
        if plp.time_length > 0 && blocks * plp.coding.cells() > TIME_CAPACITY {
            return Err(DecodeError::Capacity);
        }
        self.received = 0;
        self.next_frame = Some((post.frame + plp.frame_interval) % pre.frames);
        self.selected = Some(plp);
        self.active = true;
        Ok(())
    }

    pub fn push(
        &mut self,
        address: usize,
        cells: &[Complex<f32>],
        gains: &[f32],
        noise: f32,
        packets: &mut Vec<[u8; PACKET]>,
    ) -> Result<(), DecodeError> {
        self.push_to(address, cells, gains, noise, packets)
    }

    pub(super) fn push_to<S: Sink>(
        &mut self,
        address: usize,
        cells: &[Complex<f32>],
        gains: &[f32],
        noise: f32,
        packets: &mut S,
    ) -> Result<(), DecodeError> {
        if gains.len() != cells.len() {
            return Err(DecodeError::Length);
        }
        if !self.active || self.slice_length == 0 {
            return Ok(());
        }
        let plp = self.selected.ok_or(DecodeError::Plp)?;
        for slice in 0..self.slices {
            let start = plp.start + slice * self.interval;
            let end = start + self.slice_length;
            let first = address.max(start);
            let last = (address + cells.len()).min(end);
            if first >= last {
                continue;
            }
            let expected = slice * self.slice_length + first - start;
            if expected != self.received {
                return Err(DecodeError::Discontinuity);
            }
            let mut start = first - address;
            let end = last - address;
            while start < end {
                let blocks = self.block_size(plp)?;
                let required = blocks * plp.coding.cells();
                if required > self.buffered.capacity() {
                    return Err(DecodeError::Capacity);
                }
                let room = required
                    .checked_sub(self.buffered.len())
                    .filter(|&remaining| remaining > 0)
                    .ok_or(DecodeError::Discontinuity)?;
                let count = (end - start).min(room);
                let span = start..start + count;
                self.buffered.extend_from_slice(&cells[span.clone()]);
                self.buffered_gains.extend_from_slice(&gains[span]);
                self.received += count;
                start += count;
                if self.buffered.len() == required {
                    self.decode_block(plp, blocks, noise, packets)?;
                    self.buffered.clear();
                    self.buffered_gains.clear();
                    self.time_block += 1;
                }
            }
        }
        Ok(())
    }

    fn block_size(&self, plp: Plp) -> Result<usize, DecodeError> {
        if plp.time_length == 0 {
            return Ok(1);
        }
        if plp.time_across_frames {
            return Ok(plp.blocks);
        }
        interleave::time_block_size(plp.blocks, plp.time_length, self.time_block)
    }

    fn decode_block<S: Sink>(
        &mut self,
        plp: Plp,
        blocks: usize,
        noise: f32,
        packets: &mut S,
    ) -> Result<(), DecodeError> {
        let count = self.buffered.len();
        if plp.time_length > 0 {
            interleave::time_deinterleave(
                &self.buffered,
                &mut self.deinterleaved[..count],
                plp.coding.cells(),
            )?;
            interleave::time_deinterleave(
                &self.buffered_gains,
                &mut self.deinterleaved_gains[..count],
                plp.coding.cells(),
            )?;
        } else {
            self.deinterleaved[..count].copy_from_slice(&self.buffered);
            self.deinterleaved_gains[..count].copy_from_slice(&self.buffered_gains);
        }
        let noise = noise.max(0.0001);
        if blocks >= PARALLEL_BLOCKS && plp.time_length != 0 && self.fec_workers.is_some() {
            self.decode_parallel(plp, blocks, noise, packets);
        } else {
            self.decode_serial(plp, 0..blocks, noise, packets);
        }
        Ok(())
    }

    fn decode_parallel<S: Sink>(&mut self, plp: Plp, blocks: usize, noise: f32, packets: &mut S) {
        let Some(mut workers) = self.fec_workers.take() else {
            self.decode_serial(plp, 0..blocks, noise, packets);
            return;
        };
        let bounds = [0, blocks.div_ceil(3), (2 * blocks).div_ceil(3), blocks];
        let cell_count = plp.coding.cells();
        let [first, second] = &mut workers;
        let submitted = [(1, first), (2, second)].map(|(index, worker)| {
            let first = bounds[index];
            let range = first * cell_count..bounds[index + 1] * cell_count;
            worker.submit(
                plp.coding,
                first,
                &self.deinterleaved[range.clone()],
                &self.deinterleaved_gains[range],
                noise,
            )
        });
        self.decode_serial(plp, 0..bounds[1], noise, packets);
        for (index, (worker, submitted)) in workers.iter_mut().zip(submitted).enumerate() {
            if let Some(job) = submitted.then(|| worker.finish()).flatten() {
                for (decoded, word) in job
                    .results
                    .iter()
                    .copied()
                    .zip(job.words.chunks_exact(plp.coding.message()))
                {
                    Self::accept_word(
                        &mut self.report,
                        &mut self.transport,
                        decoded,
                        word,
                        packets,
                    );
                }
            } else {
                self.decode_serial(plp, bounds[index + 1]..bounds[index + 2], noise, packets);
            }
        }
        self.fec_workers = Some(workers);
    }

    fn decode_serial<S: Sink>(
        &mut self,
        plp: Plp,
        indices: std::ops::Range<usize>,
        noise: f32,
        packets: &mut S,
    ) {
        let cell_count = plp.coding.cells();
        for index in indices {
            let first = index * cell_count;
            let range = first..first + cell_count;
            let block = if plp.time_length == 0 {
                self.time_block
            } else {
                index
            };
            self.keep_tap(plp, first, block);
            let decoded = self.decoders[self.decoder].2.decode_weighted(
                &self.deinterleaved[range.clone()],
                &self.deinterleaved_gains[range],
                block,
                noise,
                &mut self.word,
            );
            Self::accept_word(
                &mut self.report,
                &mut self.transport,
                decoded,
                &self.word,
                packets,
            );
        }
    }

    fn accept_word<S: Sink>(
        report: &mut Report,
        transport: &mut Transport,
        decoded: Result<Decoded, DecodeError>,
        word: &[bool],
        packets: &mut S,
    ) {
        let result = decoded.and_then(|decoded| {
            report.blocks = report.blocks.saturating_add(1);
            report.corrected_bits = report
                .corrected_bits
                .saturating_add(decoded.corrected_bits as u32);
            transport.push_to(&word[..decoded.bits], packets)
        });
        match result {
            Ok(status) => {
                report.packets = report.packets.saturating_add(status.packets as u32);
                report.errors = report
                    .errors
                    .saturating_add((status.crc_errors + status.discontinuities) as u32);
                report.last_error = if status.crc_errors > 0 {
                    Some(DecodeError::Header)
                } else if status.discontinuities > 0 {
                    Some(DecodeError::Discontinuity)
                } else {
                    None
                };
            }
            Err(error) => {
                transport.reset();
                report.errors = report.errors.saturating_add(1);
                report.last_error = Some(error);
            }
        }
    }

    /// A block the decoder rejects keeps no tap; its error surfaces from the decode after.
    fn keep_tap(&mut self, plp: Plp, first: usize, block: usize) {
        if !self.tapping {
            return;
        }
        let cells = &self.deinterleaved[first..first + plp.coding.cells()];
        if self.decoders[self.decoder]
            .2
            .derotated_into(cells, block, &mut self.tap)
            .is_ok()
        {
            self.tapping = false;
            self.tapped = Some(plp.coding.constellation);
        }
    }

    pub fn end(&mut self) -> Result<(), DecodeError> {
        if self.active && self.received != self.slices * self.slice_length {
            self.reset();
            return Err(DecodeError::Discontinuity);
        }
        self.active = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
