use sdrmm_wire::DectCapability as Cap;

use super::mac::{bit, field};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Fixed,
    Extended,
    Extended2,
    Extended3,
}

pub(crate) const PARTS: usize = 4;

impl Part {
    pub fn from_qh(qh: u8) -> Option<Self> {
        match qh {
            0x3 => Some(Self::Fixed),
            0x4 => Some(Self::Extended),
            0xC => Some(Self::Extended2),
            0xE => Some(Self::Extended3),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Fixed => 0,
            Self::Extended => 1,
            Self::Extended2 => 2,
            Self::Extended3 => 3,
        }
    }

    const fn flags(self) -> &'static [(usize, Cap)] {
        match self {
            Self::Fixed => &FIXED,
            Self::Extended => &EXTENDED,
            Self::Extended2 => &EXTENDED2,
            Self::Extended3 => &EXTENDED3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NextPart {
    pub announced: Option<bool>,
}

const FIXED: [(usize, Cap); 34] = [
    (12, Cap::ExtendedFpInfo),
    (13, Cap::DoubleDuplexBearer),
    (15, Cap::DoubleSlot),
    (16, Cap::HalfSlot),
    (17, Cap::FullSlot),
    (18, Cap::FrequencyControl),
    (19, Cap::PageRepetition),
    (20, Cap::CoSetupOnDummy),
    (21, Cap::ClUplink),
    (22, Cap::ClDownlink),
    (23, Cap::BasicAFieldSetup),
    (24, Cap::AdvancedAFieldSetup),
    (25, Cap::BFieldSetup),
    (26, Cap::CfMessages),
    (27, Cap::InMinimumDelay),
    (28, Cap::InNormalDelay),
    (29, Cap::IpErrorDetection),
    (30, Cap::IpErrorCorrection),
    (31, Cap::MultibearerConnections),
    (32, Cap::Adpcm),
    (33, Cap::GapBasicSpeech),
    (34, Cap::NonVoiceCircuitSwitched),
    (35, Cap::NonVoicePacketSwitched),
    (36, Cap::StandardAuthentication),
    (37, Cap::StandardCiphering),
    (38, Cap::LocationRegistration),
    (39, Cap::SimServices),
    (40, Cap::NonStaticFixedPart),
    (41, Cap::CissServices),
    (42, Cap::ClmsService),
    (43, Cap::ComsService),
    (44, Cap::AccessRightsRequests),
    (45, Cap::ExternalHandover),
    (46, Cap::ConnectionHandover),
];

const EXTENDED: [(usize, Cap); 25] = [
    (14, Cap::CrfpEncryption),
    (20, Cap::FrequencyReplacement),
    (21, Cap::MacSuspendResume),
    (22, Cap::IpqService),
    (23, Cap::ExtendedFpInfo2),
    (25, Cap::FMmsInterworking),
    (26, Cap::BasicOdap),
    (27, Cap::GmeTransport),
    (28, Cap::IpRoaming),
    (29, Cap::Ethernet),
    (30, Cap::TokenRing),
    (31, Cap::Ip),
    (32, Cap::Ppp),
    (33, Cap::V24),
    (36, Cap::RapPart1),
    (37, Cap::IsdnIntermediateSystem),
    (38, Cap::GpsSynchronized),
    (39, Cap::TpuiRegistration),
    (40, Cap::EmergencyCall),
    (41, Cap::AsymmetricBearers),
    (43, Cap::Lrms),
    (44, Cap::DataServiceProfileD),
    (45, Cap::DprsClass3Or4),
    (46, Cap::DprsClass2),
    (47, Cap::IsdnDataServices),
];

const EXTENDED2: [(usize, Cap); 24] = [
    (12, Cap::LongSlot640),
    (13, Cap::LongSlot672),
    (14, Cap::EuMux),
    (15, Cap::IpfAdvanced),
    (16, Cap::SipfChannel),
    (17, Cap::GfChannel),
    (18, Cap::UleWrsDelayedPaging),
    (22, Cap::ExtendedFpInfo3),
    (23, Cap::NoEmissionAnyCarrier),
    (24, Cap::WidebandVoice),
    (29, Cap::ExtendedWidebandVoice),
    (30, Cap::PermanentClir),
    (31, Cap::ThirdPartyConference),
    (32, Cap::IntrusionCall),
    (33, Cap::CallDeflection),
    (34, Cap::MultipleLines),
    (35, Cap::NoEmission),
    (36, Cap::NgDect5),
    (37, Cap::UNemo),
    (38, Cap::UNemoOpportunistic),
    (42, Cap::ReKeying),
    (43, Cap::Dsaa2),
    (44, Cap::Dsc2),
    (45, Cap::LightData),
];

const EXTENDED3: [(usize, Cap); 7] = [
    (13, Cap::HalfSlotSecondHalf),
    (35, Cap::WirelessMicrophone),
    (36, Cap::AudioMicrophone),
    (37, Cap::AudioLowLatencyMicrophone),
    (38, Cap::AudioSpeaker),
    (39, Cap::AudioHighResolution),
    (40, Cap::AudioGamingHeadset),
];

const PACKET_DATA: [Cap; 5] = [
    Cap::PacketData1,
    Cap::PacketData2,
    Cap::PacketData3,
    Cap::PacketData4,
    Cap::PacketData5,
];

pub(crate) fn decode(part: Part, a: u64, out: &mut Vec<Cap>) -> NextPart {
    out.clear();
    out.extend(
        part.flags()
            .iter()
            .filter(|&&(offset, _)| bit(a, offset))
            .map(|&(_, capability)| capability),
    );
    match part {
        Part::Fixed => NextPart {
            announced: Some(bit(a, 12)),
        },
        Part::Extended => {
            extended_fields(a, out);
            NextPart {
                announced: Some(bit(a, 23)),
            }
        }
        Part::Extended2 => {
            extended2_fields(a, out);
            NextPart {
                announced: Some(bit(a, 22)),
            }
        }
        Part::Extended3 => {
            modulation(a, out);
            NextPart { announced: None }
        }
    }
}

fn extended_fields(a: u64, out: &mut Vec<Cap>) {
    if field(a, 15, 3) == 0b001 {
        out.push(Cap::RelayV2);
    }
    if field(a, 18, 2) == 0b01 {
        out.push(Cap::ProlongedPreamble);
    }
}

fn extended2_fields(a: u64, out: &mut Vec<Cap>) {
    let category = field(a, 25, 4) as usize;
    if let Some(&capability) = category.checked_sub(1).and_then(|at| PACKET_DATA.get(at)) {
        out.push(capability);
    }
    let ule = match field(a, 39, 3) {
        0b100 => Some(Cap::UlePhase1),
        0b110 => Some(Cap::UlePhase1Revised),
        0b101 => Some(Cap::UlePhase2),
        0b111 => Some(Cap::UlePhase3),
        _ => None,
    };
    out.extend(ule);
}

fn modulation(a: u64, out: &mut Vec<Cap>) {
    let code = field(a, 14, 4);
    let level = match code >> 1 {
        0b001 if code & 1 == 0 => Some(Cap::ModulationBpsk),
        0b010 => Some(Cap::ModulationQpsk),
        0b011 => Some(Cap::Modulation8psk),
        0b100 => Some(Cap::Modulation16qam),
        0b101 => Some(Cap::Modulation64qam),
        _ => None,
    };
    if let Some(level) = level {
        out.push(level);
        if code & 1 == 1 {
            out.push(Cap::HighLevelAField);
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::DectCapability as Cap;

    use super::{NextPart, Part, decode};
    use crate::dect::mac::{a_field_crc_ok, field};

    fn caps(part: Part, a: u64) -> (Vec<Cap>, NextPart) {
        let mut out = Vec::new();
        let next = decode(part, a, &mut out);
        (out, next)
    }

    fn with_bits(qh: u64, offsets: &[usize]) -> u64 {
        offsets
            .iter()
            .fold((0x8E << 56) | (qh << 52), |acc, &offset| {
                acc | (1u64 << (63 - offset))
            })
    }

    #[test]
    fn header_codes_select_the_four_capability_messages() {
        assert_eq!(Part::from_qh(0x3), Some(Part::Fixed));
        assert_eq!(Part::from_qh(0x4), Some(Part::Extended));
        assert_eq!(Part::from_qh(0xC), Some(Part::Extended2));
        assert_eq!(Part::from_qh(0xE), Some(Part::Extended3));
        assert_eq!(Part::from_qh(0x6), None);
    }

    #[test]
    fn part_two_carries_the_dsaa2_and_dsc2_bits() {
        let (found, next) = caps(Part::Extended2, with_bits(0xC, &[42, 43, 44]));
        assert_eq!(found, vec![Cap::ReKeying, Cap::Dsaa2, Cap::Dsc2]);
        assert_eq!(next.announced, Some(false));
        let (found, _) = caps(Part::Extended2, with_bits(0xC, &[43]));
        assert_eq!(found, vec![Cap::Dsaa2]);
    }

    #[test]
    fn part_two_decodes_packet_data_category_and_ule_phase() {
        let a = with_bits(0xC, &[27, 28, 39, 41, 22]);
        let (found, next) = caps(Part::Extended2, a);
        assert!(found.contains(&Cap::PacketData3), "{found:?}");
        assert!(found.contains(&Cap::UlePhase2), "{found:?}");
        assert!(found.contains(&Cap::ExtendedFpInfo3));
        assert_eq!(next.announced, Some(true));
    }

    #[test]
    fn part_one_reads_relay_preamble_and_the_part_two_flag() {
        let (found, next) = caps(Part::Extended, with_bits(0x4, &[17, 19, 21, 23, 40]));
        assert_eq!(
            found,
            vec![
                Cap::MacSuspendResume,
                Cap::ExtendedFpInfo2,
                Cap::EmergencyCall,
                Cap::RelayV2,
                Cap::ProlongedPreamble
            ]
        );
        assert_eq!(next.announced, Some(true));
    }

    #[test]
    fn part_three_reports_the_highest_modulation_and_the_a_field_option() {
        let (found, _) = caps(Part::Extended3, with_bits(0xE, &[14, 17]));
        assert_eq!(found, vec![Cap::Modulation16qam, Cap::HighLevelAField]);
        let (found, _) = caps(Part::Extended3, with_bits(0xE, &[14, 16]));
        assert_eq!(found, vec![Cap::Modulation64qam]);
        let (found, _) = caps(Part::Extended3, with_bits(0xE, &[16]));
        assert_eq!(found, vec![Cap::ModulationBpsk]);
        let (found, _) = caps(Part::Extended3, with_bits(0xE, &[16, 17]));
        assert!(found.is_empty(), "0011 is reserved, got {found:?}");
    }

    #[test]
    fn real_base_stations_off_air_announce_part_two_without_dsaa2() {
        let extended = 0x8E40_0100_0000_B158u64;
        let part_two = 0x8EC0_0000_1000_07C1u64;
        assert!(a_field_crc_ok(extended) && a_field_crc_ok(part_two));
        assert_eq!(field(extended, 8, 4), 0x4);
        assert_eq!(field(part_two, 8, 4), 0xC);
        let (found, next) = caps(Part::Extended, extended);
        assert_eq!(found, vec![Cap::ExtendedFpInfo2]);
        assert_eq!(next.announced, Some(true));
        let (found, next) = caps(Part::Extended2, part_two);
        assert_eq!(found, vec![Cap::NoEmission]);
        assert_eq!(next.announced, Some(false));
    }

    #[test]
    fn the_off_air_fixed_part_capabilities_announce_extended_info() {
        let fixed = 0x8E38_5110_CE00_DC36u64;
        assert!(a_field_crc_ok(fixed));
        let (found, next) = caps(Part::Fixed, fixed);
        assert_eq!(next.announced, Some(true));
        for expected in [
            Cap::ExtendedFpInfo,
            Cap::FullSlot,
            Cap::Adpcm,
            Cap::GapBasicSpeech,
            Cap::StandardAuthentication,
            Cap::StandardCiphering,
            Cap::LocationRegistration,
        ] {
            assert!(
                found.contains(&expected),
                "{expected:?} missing from {found:?}"
            );
        }
    }
}
