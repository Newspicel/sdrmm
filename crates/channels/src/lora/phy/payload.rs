use sdrmm_wire::{LoraCodingRate, LoraIntegrity};

use super::{
    Frame,
    chirp::Slope,
    code::{
        HEADER_NIBBLES, HEADER_PARITY_BITS, HEADER_SYMBOLS, Header, Shape, dewhiten, diagonal,
        low_data_rate, payload_crc, received_crc,
    },
    filter::Window,
    receiver::{Settings, Tools},
    ring::Ring,
    soft::{MAX_BITS, Statistics, decode_codeword},
};

const TRACK_RATIO: f32 = 10.0;
const CFO_GAIN: f64 = 0.05;

#[must_use]
pub(crate) fn max_symbols(spreading_factor: u8) -> usize {
    let shape = Shape {
        spreading_factor,
        coding_rate: LoraCodingRate::Cr48,
        low_data_rate: true,
    };
    shape.symbols_for(HEADER_NIBBLES + 2 * super::code::MAX_PAYLOAD + super::code::CRC_NIBBLES)
}

#[derive(Clone, Copy, Debug, Default)]
struct Timing {
    offset: f64,
    drift: f64,
}

impl Timing {
    fn from_clock(settings: &Settings, cfo_bins: f64) -> Self {
        let sign = if settings.inverted { -1.0 } else { 1.0 };
        let drift = if settings.carrier_hz > 0.0 {
            sign * cfo_bins * settings.bandwidth_hz / settings.carrier_hz
        } else {
            0.0
        };
        Self { offset: 0.0, drift }
    }

    fn advance(&mut self) {
        self.offset -= self.drift;
    }
}

#[derive(Clone, Copy, Debug)]
struct Plan {
    header: Header,
    shape: Shape,
    total: usize,
}

pub(crate) struct Lock {
    origin: f64,
    cfo_bins: f64,
    sync_word: u8,
    statistics: Statistics,
    snr_db: f32,
    symbol: usize,
    low_data_rate: bool,
    plan: Option<Plan>,
    timing: Timing,
    corrected: u32,
}

pub(crate) struct Finished {
    pub frame: Frame,
    pub resume_chip: u64,
}

fn implicit_plan(settings: &Settings, low_data_rate: bool) -> Option<Plan> {
    let implicit = settings.implicit_header?;
    let header = Header {
        length: implicit.length,
        coding_rate: implicit.coding_rate,
        crc: implicit.crc,
    };
    let shape = Shape {
        spreading_factor: settings.spreading_factor,
        coding_rate: implicit.coding_rate,
        low_data_rate,
    };
    Some(Plan {
        header,
        shape,
        total: shape.symbols_for(header.payload_nibbles()),
    })
}

impl Lock {
    #[must_use]
    pub(crate) fn new(
        settings: &Settings,
        origin: f64,
        cfo_bins: f64,
        sync_word: u8,
        statistics: Statistics,
        snr_db: f32,
    ) -> Self {
        let low_data_rate = low_data_rate(settings.spreading_factor, settings.bandwidth_hz);
        Self {
            origin,
            cfo_bins,
            sync_word,
            statistics,
            snr_db,
            symbol: 0,
            low_data_rate,
            plan: implicit_plan(settings, low_data_rate),
            timing: Timing::from_clock(settings, cfo_bins),
            corrected: 0,
        }
    }

    fn chips(settings: &Settings) -> usize {
        1 << settings.spreading_factor
    }

    fn window(&self, settings: &Settings) -> Window {
        let n = Self::chips(settings) as f64;
        Window {
            start_chip: self.origin + self.symbol as f64 * n + self.timing.offset,
            cfo_bins: self.cfo_bins,
            inverted: settings.inverted,
        }
    }

    fn bits(&self, settings: &Settings) -> (usize, bool) {
        let sf = usize::from(settings.spreading_factor);
        if self.symbol < HEADER_SYMBOLS || self.low_data_rate {
            (sf - 2, true)
        } else {
            (sf, false)
        }
    }

    pub(crate) fn advance(
        &mut self,
        settings: &Settings,
        tools: &mut Tools,
        ring: &Ring,
    ) -> Option<Option<Finished>> {
        let window = self.window(settings);
        if window.last_wide_index(Self::chips(settings)) >= ring.end() {
            return None;
        }
        if (self.symbol + 1) * MAX_BITS > tools.llrs.len() {
            return Some(Some(self.failed(settings)));
        }
        self.demodulate(settings, tools, ring, window);
        self.symbol += 1;
        if self.symbol == HEADER_SYMBOLS && self.plan.is_none() {
            match self.read_header(settings, tools) {
                Some(plan) => self.plan = Some(plan),
                None => return Some(Some(self.failed(settings))),
            }
        }
        match self.plan {
            Some(plan) if self.symbol >= plan.total => {
                Some(Some(self.finish(settings, tools, plan)))
            }
            _ => Some(None),
        }
    }

    fn demodulate(&mut self, settings: &Settings, tools: &mut Tools, ring: &Ring, window: Window) {
        tools.extractor.extract(ring, window, &mut tools.samples);
        tools.dechirper.transform(&tools.samples, Slope::Up);
        let peak = tools.dechirper.peak();
        let noise = tools.dechirper.noise(peak.bin);
        let (bits, reduced) = self.bits(settings);
        let start = self.symbol * MAX_BITS;
        tools.demapper.llrs(
            tools.dechirper.energies(),
            Statistics {
                amplitude: self.statistics.amplitude,
                noise,
            },
            reduced,
            &mut tools.llrs[start..start + bits],
        );
        self.timing.advance();
        if peak.energy > TRACK_RATIO * noise {
            self.cfo_bins += CFO_GAIN * tools.dechirper.fraction(peak.bin);
        }
    }

    fn decode_block(
        &mut self,
        llrs: &[f32],
        first_symbol: usize,
        rows: usize,
        parity_bits: u8,
        nibbles: &mut Vec<u8>,
    ) {
        let columns = super::code::codeword_len(parity_bits);
        let mut codeword = [0.0f32; 8];
        for row in 0..rows {
            for (column, llr) in codeword[..columns].iter_mut().enumerate() {
                let symbol = first_symbol + column;
                *llr = llrs[symbol * MAX_BITS + diagonal(row, column, rows)];
            }
            let decision = decode_codeword(&codeword[..columns], parity_bits);
            self.corrected += u32::from(decision.corrected);
            nibbles.push(decision.nibble);
        }
    }

    fn header_nibbles(&mut self, settings: &Settings, llrs: &[f32]) -> Vec<u8> {
        let rows = usize::from(settings.spreading_factor) - 2;
        let mut nibbles = Vec::with_capacity(rows);
        self.decode_block(llrs, 0, rows, HEADER_PARITY_BITS, &mut nibbles);
        nibbles
    }

    fn read_header(&mut self, settings: &Settings, tools: &Tools) -> Option<Plan> {
        let nibbles = self.header_nibbles(settings, &tools.llrs);
        let header = Header::parse(&nibbles)?;
        let shape = Shape {
            spreading_factor: settings.spreading_factor,
            coding_rate: header.coding_rate,
            low_data_rate: self.low_data_rate,
        };
        Some(Plan {
            header,
            shape,
            total: shape.symbols_for(HEADER_NIBBLES + header.payload_nibbles()),
        })
    }

    fn nibbles(&mut self, settings: &Settings, tools: &Tools, plan: Plan) -> Vec<u8> {
        self.corrected = 0;
        let mut nibbles = self.header_nibbles(settings, &tools.llrs);
        let parity = plan.shape.coding_rate.parity_bits();
        let mut symbol = HEADER_SYMBOLS;
        while symbol + plan.shape.columns() <= plan.total {
            self.decode_block(&tools.llrs, symbol, plan.shape.rows(), parity, &mut nibbles);
            symbol += plan.shape.columns();
        }
        if settings.implicit_header.is_none() {
            nibbles.drain(..HEADER_NIBBLES.min(nibbles.len()));
        }
        nibbles
    }

    fn finish(&mut self, settings: &Settings, tools: &Tools, plan: Plan) -> Finished {
        let nibbles = self.nibbles(settings, tools, plan);
        let length = usize::from(plan.header.length);
        let payload = dewhiten(&nibbles, length);
        let integrity = if !plan.header.crc {
            LoraIntegrity::NoCrc
        } else if received_crc(&nibbles, length) == Some(payload_crc(&payload)) {
            LoraIntegrity::CrcOk
        } else {
            LoraIntegrity::CrcFailed
        };
        let frame = self.frame(settings, plan.header.coding_rate, integrity, payload);
        self.finished(settings, frame)
    }

    fn failed(&self, settings: &Settings) -> Finished {
        let frame = self.frame(
            settings,
            LoraCodingRate::default(),
            LoraIntegrity::HeaderFailed,
            Vec::new(),
        );
        self.finished(settings, frame)
    }

    fn frame(
        &self,
        settings: &Settings,
        coding_rate: LoraCodingRate,
        integrity: LoraIntegrity,
        payload: Vec<u8>,
    ) -> Frame {
        let n = Self::chips(settings) as f64;
        let sign = if settings.inverted { -1.0 } else { 1.0 };
        Frame {
            spreading_factor: settings.spreading_factor,
            coding_rate,
            sync_word: self.sync_word,
            implicit_header: settings.implicit_header.is_some(),
            low_data_rate: self.low_data_rate,
            inverted_iq: settings.inverted,
            integrity,
            fec_corrected: self.corrected,
            snr_db: self.snr_db,
            frequency_error_hz: (sign * self.cfo_bins * settings.bandwidth_hz / n) as f32,
            payload,
        }
    }

    fn finished(&self, settings: &Settings, frame: Frame) -> Finished {
        let n = Self::chips(settings) as f64;
        let end = self.origin + self.symbol as f64 * n + self.timing.offset;
        Finished {
            frame,
            resume_chip: end.max(0.0).ceil() as u64,
        }
    }
}
