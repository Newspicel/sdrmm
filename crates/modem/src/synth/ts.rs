use std::collections::VecDeque;

use crate::{
    dvb::PACKET,
    mpegts::{TsWriter, null_packet, pat, pmt, sdt},
};

pub const PROGRAM_NAME: &str = "Rust TV";
pub const PROVIDER: &str = "SDRmm";

const TABLE_PERIOD: usize = 40;

/// A transport stream of one programme, a test pattern with a tone, its tables repeated.
pub struct Multiplex {
    writer: TsWriter,
    queued: VecDeque<[u8; PACKET]>,
    emitted: usize,
    tabled: usize,
    frame: u32,
}

impl Multiplex {
    #[must_use]
    pub fn new() -> Self {
        Self {
            writer: TsWriter::new(),
            queued: VecDeque::new(),
            emitted: 0,
            tabled: 0,
            frame: 0,
        }
    }

    pub fn packet(&mut self) -> [u8; PACKET] {
        if self.queued.is_empty() {
            let mut batch = Vec::new();
            let pts = 90_000 + u64::from(self.frame) * 43_200;
            let video = include_bytes!("../../../../fixtures/broadcast_audio/pattern.m2v");
            let mut starts = Vec::new();
            let mut presentation = Vec::new();
            let mut gop = 0u64;
            for (at, header) in video.windows(8).enumerate() {
                if header[..4] == [0, 0, 1, 0xb8] {
                    let code = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
                    gop = u64::from((code >> 7) & 63);
                } else if header[..4] == [0, 0, 1, 0] {
                    starts.push(if starts.is_empty() { 0 } else { at });
                    let temporal = u64::from(u16::from_be_bytes([header[4], header[5]]) >> 6);
                    presentation.push((gop + temporal) * 3600);
                }
            }
            starts.push(video.len());
            let audio = include_bytes!("../../../../fixtures/broadcast_audio/tone_48k_mono.mp2");
            let mut audio_frame = 0;
            for (index, span) in starts.windows(2).enumerate() {
                self.writer.pes(
                    0x0101,
                    0xe0,
                    pts + presentation[index],
                    &video[span[0]..span[1]],
                    &mut batch,
                );
                while audio_frame < 20 && audio_frame * 2160 < (index + 1) * 3600 {
                    self.writer.pes(
                        0x0102,
                        0xc0,
                        pts + audio_frame as u64 * 2160,
                        &audio[audio_frame * 192..(audio_frame + 1) * 192],
                        &mut batch,
                    );
                    audio_frame += 1;
                }
            }
            self.frame += 1;
            self.queued.extend(batch);
        }
        if self.emitted == 0 || self.emitted >= self.tabled + TABLE_PERIOD {
            self.tabled = self.emitted;
            let mut tables = Vec::new();
            self.writer.section(0x0000, &pat(), &mut tables);
            self.writer.section(0x0100, &pmt(), &mut tables);
            self.writer
                .section(0x0011, &sdt(PROVIDER, PROGRAM_NAME), &mut tables);
            for packet in tables.into_iter().rev() {
                self.queued.push_front(packet);
            }
        }
        self.emitted += 1;
        self.queued.pop_front().unwrap_or_else(null_packet)
    }
}

impl Default for Multiplex {
    fn default() -> Self {
        Self::new()
    }
}
