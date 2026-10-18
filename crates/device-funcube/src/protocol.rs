use std::ops::RangeInclusive;

use crate::error::{Error, Result};

pub(crate) const REPORT_LEN: usize = 64;
pub(crate) const SAMPLE_RATE_HZ: u32 = 192_000;
pub(crate) const MAX_IF_GAIN_DB: u8 = 59;
pub(crate) const BANDS_HZ: [RangeInclusive<u32>; 2] =
    [150_000..=260_000_000, 410_000_000..=2_050_000_000];

const STATUS_OK: u8 = 1;
const FIRMWARE_PREFIX: &str = "FCDAPP";

pub(crate) type Report = [u8; REPORT_LEN];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Command {
    Query = 1,
    SetFrequencyHz = 101,
    GetFrequencyHz = 102,
    SetLna = 110,
    SetRfFilter = 113,
    SetMixer = 114,
    SetIfGain = 117,
    SetIfFilter = 122,
    SetBiasTee = 126,
    GetLna = 150,
    GetRfFilter = 153,
    GetMixer = 154,
    GetIfGain = 157,
    GetIfFilter = 162,
    GetBiasTee = 166,
}

impl Command {
    pub(crate) const ALL: [Self; 15] = [
        Self::Query,
        Self::SetFrequencyHz,
        Self::GetFrequencyHz,
        Self::SetLna,
        Self::SetRfFilter,
        Self::SetMixer,
        Self::SetIfGain,
        Self::SetIfFilter,
        Self::SetBiasTee,
        Self::GetLna,
        Self::GetRfFilter,
        Self::GetMixer,
        Self::GetIfGain,
        Self::GetIfFilter,
        Self::GetBiasTee,
    ];

    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|command| *command as u8 == byte)
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Query => "query firmware",
            Self::SetFrequencyHz => "set frequency",
            Self::GetFrequencyHz => "read frequency",
            Self::SetLna => "set LNA",
            Self::SetRfFilter => "set RF filter",
            Self::SetMixer => "set mixer",
            Self::SetIfGain => "set IF gain",
            Self::SetIfFilter => "set IF filter",
            Self::SetBiasTee => "set bias tee",
            Self::GetLna => "read LNA",
            Self::GetRfFilter => "read RF filter",
            Self::GetMixer => "read mixer",
            Self::GetIfGain => "read IF gain",
            Self::GetIfFilter => "read IF filter",
            Self::GetBiasTee => "read bias tee",
        }
    }
}

pub(crate) fn request(command: Command, payload: &[u8]) -> Report {
    let mut report = [0; REPORT_LEN];
    report[0] = command as u8;
    for (slot, byte) in report[1..].iter_mut().zip(payload) {
        *slot = *byte;
    }
    report
}

pub(crate) fn name_of(byte: u8) -> &'static str {
    Command::from_byte(byte).map_or("unknown command", Command::name)
}

pub(crate) fn answers(command: Command, reply: &Report) -> bool {
    reply[0] == command as u8
}

pub(crate) fn payload(command: Command, reply: &Report) -> Result<&[u8]> {
    if !answers(command, reply) {
        return Err(Error::protocol(
            command.name(),
            "the reply answers another command",
        ));
    }
    if reply[1] != STATUS_OK {
        return Err(Error::Rejected(command));
    }
    Ok(&reply[2..])
}

pub(crate) fn decode_u32(command: Command, data: &[u8]) -> Result<u32> {
    data.first_chunk::<4>()
        .map(|bytes| u32::from_le_bytes(*bytes))
        .ok_or(Error::protocol(command.name(), "short reply"))
}

#[cfg(test)]
pub(crate) fn decode_u8(command: Command, data: &[u8]) -> Result<u8> {
    data.first()
        .copied()
        .ok_or(Error::protocol(command.name(), "short reply"))
}

pub(crate) fn decode_firmware(data: &[u8]) -> Result<String> {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let text = String::from_utf8_lossy(&data[..end]).trim().to_string();
    if !text.starts_with(FIRMWARE_PREFIX) {
        return Err(Error::protocol(
            Command::Query.name(),
            "the dongle is in its bootloader, not running the receiver firmware",
        ));
    }
    Ok(text)
}

pub(crate) fn in_band(frequency_hz: u32) -> bool {
    BANDS_HZ.iter().any(|band| band.contains(&frequency_hz))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum RfFilter {
    Mhz0To4,
    Mhz4To8,
    Mhz8To16,
    Mhz16To32,
    Mhz32To75,
    Mhz75To125,
    Mhz125To250,
    Mhz145,
    Mhz410To875,
    Mhz435,
    Mhz875To2000,
}

impl RfFilter {
    pub(crate) const ALL: [Self; 11] = [
        Self::Mhz0To4,
        Self::Mhz4To8,
        Self::Mhz8To16,
        Self::Mhz16To32,
        Self::Mhz32To75,
        Self::Mhz75To125,
        Self::Mhz125To250,
        Self::Mhz145,
        Self::Mhz410To875,
        Self::Mhz435,
        Self::Mhz875To2000,
    ];

    #[cfg(test)]
    pub(crate) fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|filter| filter.id() == id)
    }

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Mhz0To4 => "0_4",
            Self::Mhz4To8 => "4_8",
            Self::Mhz8To16 => "8_16",
            Self::Mhz16To32 => "16_32",
            Self::Mhz32To75 => "32_75",
            Self::Mhz75To125 => "75_125",
            Self::Mhz125To250 => "125_250",
            Self::Mhz145 => "145",
            Self::Mhz410To875 => "410_875",
            Self::Mhz435 => "435",
            Self::Mhz875To2000 => "875_2000",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Mhz0To4 => "0 to 4 MHz",
            Self::Mhz4To8 => "4 to 8 MHz",
            Self::Mhz8To16 => "8 to 16 MHz",
            Self::Mhz16To32 => "16 to 32 MHz",
            Self::Mhz32To75 => "32 to 75 MHz",
            Self::Mhz75To125 => "75 to 125 MHz",
            Self::Mhz125To250 => "125 to 250 MHz",
            Self::Mhz145 => "145 MHz",
            Self::Mhz410To875 => "410 to 875 MHz",
            Self::Mhz435 => "435 MHz",
            Self::Mhz875To2000 => "875 to 2000 MHz",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum IfFilter {
    Khz200,
    Khz300,
    Khz600,
    Khz1536,
    Mhz5,
    Mhz6,
    Mhz7,
    Mhz8,
}

impl IfFilter {
    pub(crate) const ALL: [Self; 8] = [
        Self::Khz200,
        Self::Khz300,
        Self::Khz600,
        Self::Khz1536,
        Self::Mhz5,
        Self::Mhz6,
        Self::Mhz7,
        Self::Mhz8,
    ];

    #[cfg(test)]
    pub(crate) fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    pub(crate) fn from_hz(hz: f64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|filter| (filter.hz() - hz).abs() < 1.0)
    }

    pub(crate) const fn hz(self) -> f64 {
        match self {
            Self::Khz200 => 200e3,
            Self::Khz300 => 300e3,
            Self::Khz600 => 600e3,
            Self::Khz1536 => 1.536e6,
            Self::Mhz5 => 5e6,
            Self::Mhz6 => 6e6,
            Self::Mhz7 => 7e6,
            Self::Mhz8 => 8e6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(command: Command, status: u8, data: &[u8]) -> Report {
        let mut report = [0; REPORT_LEN];
        report[0] = command as u8;
        report[1] = status;
        report[2..2 + data.len()].copy_from_slice(data);
        report
    }

    #[test]
    fn command_numbers_match_the_pro_plus_firmware() {
        for (command, number) in [
            (Command::Query, 1),
            (Command::SetFrequencyHz, 101),
            (Command::GetFrequencyHz, 102),
            (Command::SetLna, 110),
            (Command::SetRfFilter, 113),
            (Command::SetMixer, 114),
            (Command::SetIfGain, 117),
            (Command::SetIfFilter, 122),
            (Command::SetBiasTee, 126),
            (Command::GetLna, 150),
            (Command::GetRfFilter, 153),
            (Command::GetMixer, 154),
            (Command::GetIfGain, 157),
            (Command::GetIfFilter, 162),
            (Command::GetBiasTee, 166),
        ] {
            assert_eq!(command as u8, number, "{command:?}");
            assert_eq!(Command::from_byte(number), Some(command));
        }
        assert_eq!(name_of(99), "unknown command");
    }

    #[test]
    fn a_frequency_travels_little_endian_after_the_command() {
        let report = request(Command::SetFrequencyHz, &97_300_000u32.to_le_bytes());
        assert_eq!(report[..5], [101, 0x20, 0xae, 0xcc, 0x05]);
        assert!(report[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn a_payload_longer_than_the_report_is_cut_rather_than_overflowing() {
        let report = request(Command::SetIfGain, &[7; 100]);
        assert_eq!(report.len(), REPORT_LEN);
        assert_eq!(report[0], Command::SetIfGain as u8);
    }

    #[test]
    fn a_reply_carries_its_data_after_the_echo_and_status() {
        let answer = reply(Command::GetFrequencyHz, 1, &97_300_000u32.to_le_bytes());
        let data = payload(Command::GetFrequencyHz, &answer).expect("accepted");
        assert_eq!(
            decode_u32(Command::GetFrequencyHz, data).expect("u32"),
            97_300_000
        );
    }

    #[test]
    fn a_zero_status_is_the_firmware_refusing() {
        let answer = reply(Command::GetIfGain, 0, &[]);
        assert!(matches!(
            payload(Command::GetIfGain, &answer),
            Err(Error::Rejected(Command::GetIfGain))
        ));
    }

    #[test]
    fn a_reply_to_another_command_is_not_taken_for_this_one() {
        let answer = reply(Command::GetLna, 1, &[1]);
        assert!(!answers(Command::GetMixer, &answer));
        assert!(payload(Command::GetMixer, &answer).is_err());
    }

    #[test]
    fn the_firmware_string_stops_at_its_terminator() {
        let mut data = b"FCDAPP 20.03 Brd 2.0 No blk\0".to_vec();
        data.extend_from_slice(&[0xff; 8]);
        assert_eq!(
            decode_firmware(&data).expect("firmware"),
            "FCDAPP 20.03 Brd 2.0 No blk"
        );
    }

    #[test]
    fn a_bootloader_is_not_mistaken_for_the_receiver() {
        assert!(decode_firmware(b"FCDBL 2.0\0").is_err());
        assert!(decode_firmware(&[]).is_err());
    }

    #[test]
    fn short_replies_are_errors_not_zeroes() {
        assert!(decode_u32(Command::GetFrequencyHz, &[1, 2]).is_err());
        assert!(decode_u8(Command::GetLna, &[]).is_err());
    }

    #[test]
    fn both_tuning_windows_hold_their_edges_and_the_gap_stays_a_gap() {
        for (hz, ok) in [
            (149_999, false),
            (150_000, true),
            (260_000_000, true),
            (300_000_000, false),
            (410_000_000, true),
            (2_050_000_000, true),
            (2_050_000_001, false),
        ] {
            assert_eq!(in_band(hz), ok, "{hz} Hz");
        }
    }

    #[test]
    fn rf_filters_follow_the_firmware_order() {
        assert_eq!(RfFilter::from_index(5), Some(RfFilter::Mhz75To125));
        assert_eq!(RfFilter::from_index(10), Some(RfFilter::Mhz875To2000));
        assert_eq!(RfFilter::from_index(11), None);
        for filter in RfFilter::ALL {
            assert_eq!(RfFilter::from_id(filter.id()), Some(filter));
            assert_eq!(RfFilter::from_index(filter as u8), Some(filter));
        }
    }

    #[test]
    fn if_filters_follow_the_firmware_order() {
        assert_eq!(IfFilter::from_index(0), Some(IfFilter::Khz200));
        assert_eq!(IfFilter::from_index(3), Some(IfFilter::Khz1536));
        assert_eq!(IfFilter::from_index(8), None);
        assert_eq!(IfFilter::from_hz(1_536_000.0), Some(IfFilter::Khz1536));
        assert_eq!(IfFilter::from_hz(250_000.0), None);
        for filter in IfFilter::ALL {
            assert_eq!(IfFilter::from_index(filter as u8), Some(filter));
        }
    }
}
