use std::sync::LazyLock;

use sdrmm_dsp::SyndromeDecoder;
use sdrmm_wire::{EotArming, EotBattery, EotReport, EotStatus, HotCommand, HotRequest};

pub(crate) const REAR_SYNC: u64 = 0x5_5555_5712;
pub(crate) const REAR_SYNC_BITS: u32 = 35;
pub(crate) const HEAD_SYNC: u64 = 0x5555_558F_1129;
pub(crate) const HEAD_SYNC_BITS: u32 = 48;
pub(crate) const HEAD_COPIES: usize = 3;
pub(crate) const HEAD_BLOCK_BITS: usize = 64;
pub(crate) const REAR_BITS: usize = REAR.bits();

const REAR_KEY: u64 = 0x2_B770;
const REAR_GENERATOR: u64 = 0x7_9A0F;
const HEAD_GENERATOR: u64 = 0x3_9B5C_2CFB;
const REAR_CORRECTABLE: u32 = 2;
const HEAD_CORRECTABLE: u32 = 3;
const STATUS_REQUEST: u8 = 0x55;
const EMERGENCY: u8 = 0xAA;
const ARMING_TYPE: u8 = 7;

static REAR_CODE: LazyLock<SyndromeDecoder> =
    LazyLock::new(|| SyndromeDecoder::new(REAR_GENERATOR, 63, REAR_CORRECTABLE));
static HEAD_CODE: LazyLock<SyndromeDecoder> =
    LazyLock::new(|| SyndromeDecoder::new(HEAD_GENERATOR, 63, HEAD_CORRECTABLE));

pub(crate) fn prepare() {
    LazyLock::force(&REAR_CODE);
    LazyLock::force(&HEAD_CODE);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    data_bits: u32,
    parity_bits: u32,
}

pub(crate) const REAR: Layout = Layout {
    data_bits: 45,
    parity_bits: 18,
};

pub(crate) const HEAD: Layout = Layout {
    data_bits: 30,
    parity_bits: 33,
};

impl Layout {
    pub(crate) const fn bits(self) -> usize {
        (self.data_bits + self.parity_bits) as usize
    }

    pub(crate) const fn position(self, sent: usize) -> u32 {
        let sent = sent as u32;
        if sent < self.data_bits {
            self.parity_bits + sent
        } else {
            self.parity_bits - 1 - (sent - self.data_bits)
        }
    }

    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn sent(self, word: u64) -> impl Iterator<Item = bool> {
        (0..self.bits()).map(move |sent| word >> self.position(sent) & 1 == 1)
    }
}

#[derive(Clone, Copy)]
struct Field {
    at: u32,
    width: u32,
}

impl Field {
    const fn get(self, data: u64) -> u32 {
        (data >> self.at & ((1 << self.width) - 1)) as u32
    }

    const fn flag(self, data: u64) -> bool {
        self.get(data) == 1
    }

    #[cfg(any(test, feature = "synth"))]
    const fn put(self, value: u32) -> u64 {
        (value as u64 & ((1 << self.width) - 1)) << self.at
    }
}

const fn field(at: u32, width: u32) -> Field {
    Field { at, width }
}

const CHAINING: Field = field(0, 2);
const BATTERY: Field = field(2, 2);
const MESSAGE_TYPE: Field = field(4, 3);
const REAR_ADDRESS: Field = field(7, 17);
const PRESSURE: Field = field(24, 7);
const DISCRETIONARY: Field = field(31, 1);
const CHARGE: Field = field(32, 7);
const VALVE: Field = field(39, 1);
const CONFIRMED: Field = field(40, 1);
const TURBINE: Field = field(41, 1);
const MOTION: Field = field(42, 1);
const MARKER_BATTERY: Field = field(43, 1);
const MARKER_LIGHT: Field = field(44, 1);
const HEAD_ADDRESS: Field = field(5, 17);
const COMMAND: Field = field(22, 8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decoded {
    pub(crate) unit_address: u32,
    pub(crate) report: EotReport,
    pub(crate) errors: u32,
}

pub(crate) fn rear(word: u64) -> Option<Decoded> {
    let (fixed, errors) = REAR_CODE.decode(word ^ REAR_KEY)?;
    let data = fixed >> REAR.parity_bits;
    Some(Decoded {
        unit_address: REAR_ADDRESS.get(data),
        report: EotReport::Rear(status(data)),
        errors,
    })
}

fn status(data: u64) -> EotStatus {
    let message_type = MESSAGE_TYPE.get(data) as u8;
    let confirmed = CONFIRMED.flag(data);
    EotStatus {
        message_type,
        arming: match (message_type, confirmed) {
            (ARMING_TYPE, false) => EotArming::Arming,
            (ARMING_TYPE, true) => EotArming::Armed,
            _ => EotArming::Normal,
        },
        pressure_psig: PRESSURE.get(data) as u8,
        battery: match BATTERY.get(data) {
            0 => EotBattery::NotMonitored,
            1 => EotBattery::VeryLow,
            2 => EotBattery::Low,
            _ => EotBattery::Ok,
        },
        battery_charge_pct: ((CHARGE.get(data) * 200 + 127) / 254) as u8,
        valve_ok: VALVE.flag(data),
        confirmed,
        turbine: TURBINE.flag(data),
        motion: MOTION.flag(data),
        marker_light: MARKER_LIGHT.flag(data),
        marker_battery_low: MARKER_BATTERY.flag(data),
        discretionary: DISCRETIONARY.flag(data),
        chaining: CHAINING.get(data) as u8,
    }
}

pub(crate) fn hard(layout: Layout, soft: &[f32]) -> u64 {
    soft.iter()
        .take(layout.bits())
        .enumerate()
        .fold(0, |word, (sent, &value)| {
            word | u64::from(value > 0.0) << layout.position(sent)
        })
}

pub(crate) fn head(summed: u64, copies: &[u64; HEAD_COPIES]) -> Option<Decoded> {
    let [a, b, c] = *copies;
    let voted = a & b | a & c | b & c;
    let (fixed, errors) = [summed, voted, a, b, c]
        .into_iter()
        .filter_map(|word| HEAD_CODE.decode(word))
        .min_by_key(|&(_, errors)| errors)?;
    let agreeing = copies
        .iter()
        .filter(|&&copy| {
            HEAD_CODE
                .decode(copy)
                .is_some_and(|(word, _)| word == fixed)
        })
        .count();
    let data = fixed >> HEAD.parity_bits;
    let code = COMMAND.get(data) as u8;
    Some(Decoded {
        unit_address: HEAD_ADDRESS.get(data),
        report: EotReport::Head(HotRequest {
            command: match code {
                STATUS_REQUEST => HotCommand::StatusRequest,
                EMERGENCY => HotCommand::Emergency,
                _ => HotCommand::Other,
            },
            code,
            copies: agreeing as u8,
        }),
        errors,
    })
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn rear_codeword(unit_address: u32, status: &EotStatus) -> u64 {
    let battery = match status.battery {
        EotBattery::NotMonitored => 0,
        EotBattery::VeryLow => 1,
        EotBattery::Low => 2,
        EotBattery::Ok => 3,
    };
    let charge = (u32::from(status.battery_charge_pct.min(100)) * 254 + 100) / 200;
    let data = CHAINING.put(u32::from(status.chaining))
        | BATTERY.put(battery)
        | MESSAGE_TYPE.put(u32::from(status.message_type))
        | REAR_ADDRESS.put(unit_address)
        | PRESSURE.put(u32::from(status.pressure_psig))
        | DISCRETIONARY.put(u32::from(status.discretionary))
        | CHARGE.put(charge)
        | VALVE.put(u32::from(status.valve_ok))
        | CONFIRMED.put(u32::from(status.confirmed))
        | TURBINE.put(u32::from(status.turbine))
        | MOTION.put(u32::from(status.motion))
        | MARKER_BATTERY.put(u32::from(status.marker_battery_low))
        | MARKER_LIGHT.put(u32::from(status.marker_light));
    REAR_CODE.encode(data) ^ REAR_KEY
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn head_codeword(unit_address: u32, code: u8) -> u64 {
    HEAD_CODE.encode(HEAD_ADDRESS.put(unit_address) | COMMAND.put(u32::from(code)))
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn odd_parity(word: u64) -> bool {
    word.count_ones().is_multiple_of(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(bits: &str) -> u64 {
        bits.bytes().enumerate().fold(0, |word, (sent, bit)| {
            word | u64::from(bit == b'1') << REAR.position(sent)
        })
    }

    fn status() -> EotStatus {
        EotStatus {
            message_type: 7,
            arming: EotArming::Armed,
            pressure_psig: 88,
            battery: EotBattery::Low,
            battery_charge_pct: 63,
            valve_ok: true,
            confirmed: true,
            turbine: false,
            motion: true,
            marker_light: true,
            marker_battery_low: false,
            discretionary: false,
            chaining: 3,
        }
    }

    #[test]
    fn the_sync_words_end_the_dotting_with_the_published_frame_sync() {
        assert_eq!(REAR_SYNC & 0x7FF, 0b111_0001_0010);
        assert_eq!(HEAD_SYNC & 0xFF_FFFF, 0b1000_1111_0001_0001_0010_1001);
        assert_eq!(REAR_SYNC >> 11, 0xAA_AAAA);
        assert_eq!(HEAD_SYNC >> 24, 0x55_5555);
    }

    #[test]
    fn the_cipher_key_matches_the_check_bits_of_an_all_zero_block() {
        assert_eq!(
            packet(&format!("{}{}", "0".repeat(45), "101011011101110000")),
            REAR_KEY
        );
        assert_eq!(rear(REAR_KEY).map(|d| d.errors), Some(0));
    }

    #[test]
    fn a_rear_block_survives_two_flipped_bits() {
        let word = rear_codeword(12_345, &status());
        let expected = Decoded {
            unit_address: 12_345,
            report: EotReport::Rear(status()),
            errors: 0,
        };
        assert_eq!(rear(word), Some(expected.clone()));
        assert_eq!(
            rear(word ^ (1 << 3) ^ (1 << 50)),
            Some(Decoded {
                errors: 2,
                ..expected
            })
        );
        assert_eq!(rear(word ^ 0b111 << 20), None);
    }

    #[test]
    fn the_charge_scale_round_trips_every_percent() {
        for pct in 0..=100 {
            let mut sent = status();
            sent.battery_charge_pct = pct;
            let Some(Decoded {
                report: EotReport::Rear(got),
                ..
            }) = rear(rear_codeword(1, &sent))
            else {
                panic!("block {pct} did not decode");
            };
            assert_eq!(got.battery_charge_pct, pct);
        }
    }

    #[test]
    fn head_copies_vote_out_a_burst_in_one_copy() {
        let clean = head_codeword(54_321, EMERGENCY);
        let copies = [clean ^ 0xFF_FF00, clean ^ 1 << 40, clean];
        let decoded = head(clean ^ 0b11_1111, &copies).expect("vote decodes");
        assert_eq!(decoded.unit_address, 54_321);
        assert_eq!(decoded.errors, 0);
        assert_eq!(
            decoded.report,
            EotReport::Head(HotRequest {
                command: HotCommand::Emergency,
                code: EMERGENCY,
                copies: 2,
            })
        );
        let broken = clean ^ 0x7F;
        assert!(head(broken, &[broken, broken, broken]).is_none());
    }
}
