use std::{
    ops::Deref,
    sync::mpsc::{SyncSender, TrySendError},
};

use sdrmm_device::{Block, BlockGap, BlockPool};

use crate::{convert::Format, protocol::SAMPLE_RATE_HZ};

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod stream;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use stream::{Pump, PumpState, RawStream};

pub(crate) const FORMAT: Format = Format::Int16;
#[cfg(any(test, target_os = "linux"))]
pub(crate) const PACKET_BYTES: usize = 792;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) const PACKETS: usize = 32;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) const TRANSFERS: usize = 8;
pub(crate) const SAMPLES_PER_PACKET: u64 = SAMPLE_RATE_HZ as u64 / 1000;
pub(crate) const BLOCK_SAMPLES: usize = 12_288;
const BLOCK_BYTES: usize = BLOCK_SAMPLES * FORMAT.width();
#[cfg(any(test, target_os = "linux", target_os = "windows"))]
const QUEUE: usize = 16;

#[derive(Debug)]
pub(crate) struct Chunk {
    block: Block,
    gap: BlockGap,
}

impl Deref for Chunk {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.block
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Packet {
    pub(crate) offset: usize,
    pub(crate) length: usize,
    pub(crate) ok: bool,
}

pub(crate) struct Assembler {
    pool: BlockPool,
    block: Option<Block>,
    filled: usize,
    pending: BlockGap,
    out: SyncSender<Chunk>,
    closed: bool,
}

impl Assembler {
    pub(crate) fn new(out: SyncSender<Chunk>) -> Self {
        Self {
            pool: BlockPool::default(),
            block: None,
            filled: 0,
            pending: BlockGap::default(),
            out,
            closed: false,
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }

    pub(crate) fn transfer(&mut self, buffer: &[u8], packets: impl IntoIterator<Item = Packet>) {
        for packet in packets {
            let data = buffer.get(packet.offset..packet.offset + packet.length);
            match data {
                Some(data) if packet.ok => self.push(data),
                _ => self.lost(SAMPLES_PER_PACKET),
            }
        }
    }

    pub(crate) fn lost(&mut self, samples: u64) {
        if samples == 0 {
            return;
        }
        self.flush();
        self.pending.estimated += samples;
    }

    fn push(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let block = match &mut self.block {
                Some(block) => block,
                None => self.block.insert(self.pool.take(BLOCK_BYTES)),
            };
            let take = (BLOCK_BYTES - self.filled).min(data.len());
            block.bytes_mut()[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == BLOCK_BYTES {
                self.flush();
            }
        }
    }

    pub(crate) fn flush(&mut self) {
        let Some(mut block) = self.block.take() else {
            return;
        };
        block.truncate(self.filled);
        let samples = (self.filled / FORMAT.width()) as u64;
        self.filled = 0;
        let gap = std::mem::take(&mut self.pending);
        match self.out.try_send(Chunk { block, gap }) {
            Ok(()) => {}
            Err(TrySendError::Full(chunk)) => {
                self.pending = BlockGap {
                    exact: chunk.gap.exact + samples,
                    estimated: chunk.gap.estimated,
                };
            }
            Err(TrySendError::Disconnected(_)) => self.closed = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};

    use super::*;

    fn assembler() -> (Assembler, Receiver<Chunk>) {
        let (out, chunks) = mpsc::sync_channel(QUEUE);
        (Assembler::new(out), chunks)
    }

    fn packet_bytes(seed: u8, frames: usize) -> Vec<u8> {
        (0..frames * FORMAT.width())
            .map(|n| seed.wrapping_add(n as u8))
            .collect()
    }

    fn transfer(lengths: &[Option<usize>]) -> (Vec<u8>, Vec<Packet>) {
        let mut buffer = vec![0u8; lengths.len() * PACKET_BYTES];
        let mut packets = Vec::new();
        for (index, length) in lengths.iter().enumerate() {
            let data = packet_bytes(index as u8, length.unwrap_or(0) / FORMAT.width());
            buffer[index * PACKET_BYTES..index * PACKET_BYTES + data.len()].copy_from_slice(&data);
            packets.push(Packet {
                offset: index * PACKET_BYTES,
                length: length.unwrap_or(0),
                ok: length.is_some(),
            });
        }
        (buffer, packets)
    }

    fn drain(chunks: &Receiver<Chunk>) -> Vec<(BlockGap, Vec<u8>)> {
        std::iter::from_fn(|| chunks.try_recv().ok())
            .map(|chunk| (chunk.gap, chunk.to_vec()))
            .collect()
    }

    #[test]
    fn packets_are_joined_in_order_without_their_padding() {
        let (mut assembler, chunks) = assembler();
        let (buffer, packets) = transfer(&[Some(768), Some(764), Some(772)]);
        assembler.transfer(&buffer, packets);
        assembler.flush();
        let got = drain(&chunks);
        assert_eq!(got.len(), 1);
        let expected: Vec<u8> = [
            packet_bytes(0, 192),
            packet_bytes(1, 191),
            packet_bytes(2, 193),
        ]
        .concat();
        assert_eq!(got[0].1, expected);
        assert_eq!(got[0].0, BlockGap::default());
    }

    #[test]
    fn a_failed_packet_closes_the_block_and_marks_the_gap_on_the_next() {
        let (mut assembler, chunks) = assembler();
        let (buffer, packets) = transfer(&[Some(768), None, Some(768)]);
        assembler.transfer(&buffer, packets);
        assembler.flush();
        let got = drain(&chunks);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, BlockGap::default());
        assert_eq!(got[0].1.len(), 768);
        assert_eq!(
            got[1].0,
            BlockGap {
                exact: 0,
                estimated: SAMPLES_PER_PACKET
            }
        );
        assert_eq!(got[1].1, packet_bytes(2, 192));
    }

    #[test]
    fn full_blocks_leave_as_soon_as_they_fill() {
        let (mut assembler, chunks) = assembler();
        let lengths = vec![Some(768); 80];
        let (buffer, packets) = transfer(&lengths);
        assembler.transfer(&buffer, packets);
        let got = drain(&chunks);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1.len(), BLOCK_BYTES);
        assembler.flush();
        let rest = drain(&chunks);
        assert_eq!(rest[0].1.len(), 80 * 768 - BLOCK_BYTES);
    }

    #[test]
    fn a_chunk_nobody_took_in_time_is_counted_exactly_on_the_next() {
        let (out, chunks) = mpsc::sync_channel(1);
        let mut assembler = Assembler::new(out);
        let (buffer, packets) = transfer(&[Some(768)]);
        for _ in 0..3 {
            assembler.transfer(&buffer, packets.clone());
            assembler.flush();
        }
        let first = chunks.try_recv().expect("first");
        assert_eq!(first.gap, BlockGap::default());
        assert!(chunks.try_recv().is_err());
        assembler.transfer(&buffer, packets);
        assembler.flush();
        let after = chunks.try_recv().expect("after the drop");
        assert_eq!(after.gap.exact, 2 * 192);
    }

    #[test]
    fn a_vanished_reader_closes_the_assembler() {
        let (mut assembler, chunks) = assembler();
        drop(chunks);
        let (buffer, packets) = transfer(&[Some(768)]);
        assembler.transfer(&buffer, packets);
        assembler.flush();
        assert!(assembler.is_closed());
    }

    #[test]
    fn a_packet_reaching_past_the_buffer_counts_as_lost() {
        let (mut assembler, chunks) = assembler();
        let buffer = vec![1u8; PACKET_BYTES];
        assembler.transfer(
            &buffer,
            [Packet {
                offset: 100,
                length: PACKET_BYTES,
                ok: true,
            }],
        );
        assembler.transfer(
            &buffer,
            [Packet {
                offset: 0,
                length: 768,
                ok: true,
            }],
        );
        assembler.flush();
        let got = drain(&chunks);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.estimated, SAMPLES_PER_PACKET);
    }

    #[test]
    fn a_packet_holds_one_millisecond_of_samples() {
        assert_eq!(SAMPLES_PER_PACKET, 192);
        assert!(SAMPLES_PER_PACKET as usize * FORMAT.width() <= PACKET_BYTES);
        assert_eq!(BLOCK_BYTES % FORMAT.width(), 0);
    }
}
