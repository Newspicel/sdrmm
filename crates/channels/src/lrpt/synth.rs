use crate::lrpt::LrptMode;
use num_complex::Complex;
use sdrmm_dsp::design_rrc;
use sdrmm_modem::ccsds::{CaduWriter, Segment};
use sdrmm_modem::jpeg::{BitWriter, Tables, ZIGZAG, encode_block, forward_dct};
use sdrmm_modem::synth::{NARROW_HZ, Render, Signal, resample};

use crate::lrpt::{
    image::{FIRST_APID, IMAGE_HEADER, MCU_ID_AT, MCUS_PER_ROW, QUALITY_AT, TELEMETRY_APID, WIDTH},
    jpeg::{Block, MCU_PIXELS, MCU_SIDE, MCUS_PER_PACKET, quant_table},
    link::{
        BASIS, IMAGE_VCID, LAYOUT, PACKET_HEADER, PacketHeader, SEQUENCE_MODULO, SPACECRAFT_ID,
    },
};

pub const DEFAULT_APIDS: [u16; 3] = [64, 65, 66];
pub const QUALITY: u8 = 80;
const SYNTH_SPS: usize = 4;
const PULSE_SPAN: usize = 8;
const ROLL_OFF: f64 = 0.6;
const LEAD_FILL: usize = 4;
const TRAIL_FILL: usize = 2;
const ROW_MS: u32 = 1_200;

pub use sdrmm_modem::ccsds::Cadu;

pub struct Scene {
    pub apids: Vec<u16>,
    pub lines: usize,
    pub planes: Vec<Vec<u8>>,
}

impl Scene {
    #[must_use]
    pub fn new(apids: &[u16], rows: u16) -> Self {
        let lines = usize::from(rows) * MCU_SIDE;
        let planes = apids
            .iter()
            .map(|&apid| {
                let tilt = f64::from(apid - FIRST_APID);
                (0..lines * WIDTH)
                    .map(|index| {
                        let (x, y) = ((index % WIDTH) as f64, (index / WIDTH) as f64);
                        let value = 128.0
                            + 70.0 * (x * 0.013 + tilt).sin()
                            + 40.0 * (y * 0.09 + 0.7 * tilt).cos();
                        value.round().clamp(0.0, 255.0) as u8
                    })
                    .collect()
            })
            .collect();
        Self {
            apids: apids.to_vec(),
            lines,
            planes,
        }
    }

    #[must_use]
    pub fn plane(&self, apid: u16) -> Option<&[u8]> {
        let index = self.apids.iter().position(|&a| a == apid)?;
        Some(&self.planes[index])
    }

    fn block(&self, plane: usize, row: usize, column: usize) -> Block {
        std::array::from_fn(|index| {
            let (y, x) = (index / MCU_SIDE, index % MCU_SIDE);
            self.planes[plane][(row * MCU_SIDE + y) * WIDTH + column * MCU_SIDE + x]
        })
    }
}

fn quantised(block: &Block, quant: &[u16; MCU_PIXELS]) -> [i32; MCU_PIXELS] {
    let coefficients = forward_dct(block);
    std::array::from_fn(|k| {
        let natural = ZIGZAG[k];
        (coefficients[natural] / f32::from(quant[natural]))
            .round()
            .clamp(-1023.0, 1023.0) as i32
    })
}

#[must_use]
pub fn encode_mcus(blocks: &[Block], quality: u8) -> Vec<u8> {
    let quant = quant_table(quality);
    let mut writer = BitWriter::default();
    let mut previous_dc = 0;
    for block in blocks {
        let zigzag = quantised(block, &quant);
        let difference = (zigzag[0] - previous_dc).clamp(-2047, 2047);
        previous_dc += difference;
        encode_block(&mut writer, difference, &zigzag, Tables::luma());
    }
    writer.finish()
}

fn packet(apid: u16, sequence: u32, row: usize, user: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; PACKET_HEADER];
    let ms = row as u32 * ROW_MS;
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&ms.to_be_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(user);
    PacketHeader {
        secondary: true,
        apid,
        segment: Segment::Unsegmented,
        sequence: (sequence % SEQUENCE_MODULO) as u16,
        length: bytes.len() - PACKET_HEADER,
    }
    .write(&mut bytes);
    bytes
}

fn image_packet(scene: &Scene, plane: usize, row: usize, first: usize, quality: u8) -> Vec<u8> {
    let blocks: Vec<Block> = (first..(first + MCUS_PER_PACKET).min(MCUS_PER_ROW))
        .map(|column| scene.block(plane, row, column))
        .collect();
    let mut user = vec![0u8; IMAGE_HEADER - MCU_ID_AT];
    user[0] = first as u8;
    user[QUALITY_AT - MCU_ID_AT] = quality;
    user.extend_from_slice(&encode_mcus(&blocks, quality));
    user
}

#[must_use]
pub fn packets(scene: &Scene, quality: u8, first_sequence: u32) -> Vec<Vec<u8>> {
    let mut sequence = first_sequence;
    let mut out = Vec::new();
    for row in 0..scene.lines / MCU_SIDE {
        for (plane, &apid) in scene.apids.iter().enumerate() {
            for first in (0..MCUS_PER_ROW).step_by(MCUS_PER_PACKET) {
                let user = image_packet(scene, plane, row, first, quality);
                out.push(packet(apid, sequence, row, &user));
                sequence += 1;
            }
        }
        out.push(packet(TELEMETRY_APID, sequence, row, &[0x55; 32]));
        sequence += 1;
    }
    out
}

#[must_use]
pub fn cadus(packets: &[Vec<u8>]) -> Vec<Cadu> {
    let writer = CaduWriter::new(SPACECRAFT_ID, LAYOUT, BASIS);
    let mut out: Vec<Cadu> = (0..LEAD_FILL)
        .map(|index| writer.fill(index as u32))
        .collect();
    out.extend(writer.channel(IMAGE_VCID, 0, packets));
    out.extend((0..TRAIL_FILL).map(|index| writer.fill((LEAD_FILL + index) as u32)));
    out
}

#[must_use]
pub fn modulate(mode: LrptMode, cadus: &[Cadu], rate: f64) -> Vec<Complex<f32>> {
    let coded = sdrmm_modem::ccsds::coded_bits(cadus);
    let level = |bit: bool| if bit { 1.0f32 } else { -1.0 };
    let stagger = if mode.offset() { SYNTH_SPS / 2 } else { 0 };
    let symbols = coded.len() / 2;
    let pulse = design_rrc(SYNTH_SPS as f64, ROLL_OFF, PULSE_SPAN);
    let mut impulses = vec![Complex::new(0.0f32, 0.0); symbols * SYNTH_SPS + stagger + 1];
    for (index, &[first, second]) in coded.as_chunks::<2>().0.iter().enumerate() {
        impulses[index * SYNTH_SPS].re = level(first);
        impulses[index * SYNTH_SPS + stagger].im = level(second);
    }
    let peak = pulse.iter().copied().fold(0.0f32, f32::max);
    let mut shaped = vec![Complex::new(0.0f32, 0.0); impulses.len() + pulse.len()];
    for (index, &impulse) in impulses.iter().enumerate() {
        if impulse == Complex::new(0.0, 0.0) {
            continue;
        }
        for (offset, &tap) in pulse.iter().enumerate() {
            shaped[index + offset] += impulse * (tap / peak * 0.5);
        }
    }
    let native = mode.symbol_rate() * SYNTH_SPS as f64;
    if (native - rate).abs() < 1e-6 {
        shaped
    } else {
        sdrmm_modem::synth::resample(&shaped, native, rate)
    }
}

#[must_use]
pub fn transmission(mode: LrptMode, rows: u16, rate: f64) -> Vec<Complex<f32>> {
    let scene = Scene::new(&DEFAULT_APIDS, rows);
    modulate(mode, &cadus(&packets(&scene, QUALITY, 0)), rate)
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "lrpt",
    label: "Meteor LRPT",
    rate_hz: NARROW_HZ,
    render: Render::Always(lrpt),
}];

fn lrpt() -> Vec<Complex<f32>> {
    const RATE: f64 = 288_000.0;
    let native = transmission(LrptMode::Oqpsk72, 4, RATE);
    resample(&native, RATE, NARROW_HZ)
}
