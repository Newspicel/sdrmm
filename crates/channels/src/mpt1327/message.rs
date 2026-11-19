//! The address codewords a trunking system controller sends, read field by field as MPT 1327
//! section 5 lays them out. Bit numbers below count from 1, the first bit sent.

use super::codeword::INFO_BITS;

const IDENT_BITS: u32 = 13;

/// A radio address: a 7-bit prefix and a 13-bit ident.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    pub prefix: u8,
    pub ident: u16,
}

impl Address {
    /// The address as one 20-bit number, the prefix above the ident.
    pub(crate) fn number(self) -> u32 {
        u32::from(self.prefix) << IDENT_BITS | u32::from(self.ident)
    }

    /// The address the way MPT 1327 users write it.
    pub(crate) fn label(self) -> String {
        format!("{}/{}", self.prefix, self.ident)
    }
}

/// One address codeword.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Message {
    /// Go to channel: the called and calling parties move to a traffic channel.
    Gtc {
        called: Address,
        calling: Address,
        channel: u16,
        data: bool,
    },
    Aloha {
        function: u8,
        invited: Address,
        channel_low: u8,
    },
    Ack {
        function: u8,
        called: Address,
        calling: Address,
    },
    Ahoy {
        function: u8,
        called: Address,
        calling: Address,
        appended: bool,
    },
    Mark {
        channel_low: u8,
        system: u16,
    },
    Maint {
        unit: Address,
        channel: u16,
        operation: u8,
    },
    Clear {
        channel: u16,
        control: u16,
    },
    Move {
        units: Address,
        control: u16,
    },
    Broadcast {
        definition: u8,
        system: u16,
        channel: u16,
        adjacent_site: u8,
    },
    ShortData {
        called: Address,
        calling: Address,
        codewords: u8,
    },
    SingleAddress {
        unit: Address,
    },
    StandardData {
        unit: Address,
    },
    Unassigned {
        category: u8,
        unit: Address,
    },
}

/// Bits `first` to `first + len - 1` of the information field.
fn field(info: u64, first: u32, len: u32) -> u32 {
    (info >> (INFO_BITS + 1 - first - len) & ((1 << len) - 1)) as u32
}

fn address(info: u64, prefix_at: u32) -> Address {
    Address {
        prefix: field(info, prefix_at, 7) as u8,
        ident: field(info, prefix_at + 7, IDENT_BITS) as u16,
    }
}

fn unit(info: u64) -> Address {
    address(info, 2)
}

/// The second party in bits 31 to 43, which shares the first party's prefix.
fn second(info: u64) -> Address {
    Address {
        prefix: field(info, 2, 7) as u8,
        ident: field(info, 31, IDENT_BITS) as u16,
    }
}

impl Message {
    pub(crate) fn read(info: u64) -> Self {
        if field(info, 22, 1) == 0 {
            return Self::Gtc {
                called: unit(info),
                calling: Address {
                    prefix: field(info, 2, 7) as u8,
                    ident: field(info, 34, IDENT_BITS) as u16,
                },
                channel: field(info, 24, 10) as u16,
                data: field(info, 23, 1) == 1,
            };
        }
        match field(info, 23, 3) {
            0b000 => category_zero(info),
            0b001 if field(info, 26, 1) == 1 => Self::ShortData {
                called: unit(info),
                calling: address(info, 29),
                codewords: field(info, 27, 2) as u8 + 1,
            },
            0b001 => Self::SingleAddress { unit: unit(info) },
            0b010 | 0b101 => Self::StandardData { unit: unit(info) },
            category => Self::Unassigned {
                category: category as u8,
                unit: unit(info),
            },
        }
    }

    /// How many data codewords follow this one in the same message.
    pub(crate) const fn appended(self) -> u8 {
        match self {
            Self::ShortData { codewords, .. } => codewords,
            Self::Ahoy {
                function: AHY,
                appended: true,
                ..
            } => 1,
            _ => 0,
        }
    }

    /// The identity of the system sending this codeword, when the codeword names its own.
    pub(crate) const fn own_system(self) -> Option<u16> {
        match self {
            Self::Mark { system, .. }
            | Self::Broadcast {
                definition: CALL_MAINTENANCE_PARAMETERS,
                system,
                ..
            } => Some(system),
            _ => None,
        }
    }

    pub(crate) const fn mnemonic(self) -> &'static str {
        match self {
            Self::Gtc { .. } => GTC,
            Self::Aloha { function, .. } => ALOHAS[function as usize & 7],
            Self::Ack { function, .. } => ACKS[function as usize & 7],
            Self::Ahoy { function, .. } => AHOYS[function as usize & 7],
            Self::Mark { .. } => "MARK",
            Self::Maint { .. } => "MAINT",
            Self::Clear { .. } => "CLEAR",
            Self::Move { .. } => "MOVE",
            Self::Broadcast { .. } => "BCAST",
            Self::ShortData { .. } => "HEAD",
            Self::SingleAddress { .. } => "SAMO",
            Self::StandardData { .. } => "DATA",
            Self::Unassigned { .. } => "SPARE",
        }
    }
}

pub(crate) const GTC: &str = "GTC";
const AHY: u8 = 0b000;
const CALL_MAINTENANCE_PARAMETERS: u8 = 0b00010;

const ALOHAS: [&str; 8] = [
    "ALH", "ALHS", "ALHD", "ALHE", "ALHR", "ALHX", "ALHF", "ALH7",
];
const ACKS: [&str; 8] = [
    "ACK", "ACKI", "ACKQ", "ACKX", "ACKV", "ACKE", "ACKT", "ACKB",
];
const AHOYS: [&str; 8] = [
    "AHY", "AHY1", "AHYX", "AHY3", "AHY4", "AHYP", "AHYQ", "AHYC",
];

fn category_zero(info: u64) -> Message {
    let function = field(info, 28, 3) as u8;
    match field(info, 26, 2) {
        0b00 => Message::Aloha {
            function,
            invited: unit(info),
            channel_low: field(info, 31, 4) as u8,
        },
        0b01 => Message::Ack {
            function,
            called: unit(info),
            calling: second(info),
        },
        0b10 => Message::Ahoy {
            function,
            called: unit(info),
            calling: second(info),
            appended: field(info, 48, 1) == 1,
        },
        _ => miscellaneous(info, function),
    }
}

fn miscellaneous(info: u64, function: u8) -> Message {
    match function {
        0b000 => Message::Mark {
            channel_low: field(info, 2, 4) as u8,
            system: field(info, 7, 15) as u16,
        },
        0b001 => Message::Maint {
            unit: unit(info),
            channel: field(info, 31, 10) as u16,
            operation: field(info, 41, 3) as u8,
        },
        0b010 => Message::Clear {
            channel: field(info, 2, 10) as u16,
            control: field(info, 12, 10) as u16,
        },
        0b011 => Message::Move {
            units: unit(info),
            control: field(info, 31, 10) as u16,
        },
        0b100 => Message::Broadcast {
            definition: field(info, 2, 5) as u8,
            system: field(info, 7, 15) as u16,
            channel: field(info, 31, 10) as u16,
            adjacent_site: field(info, 45, 4) as u8,
        },
        _ => Message::Unassigned {
            category: 0,
            unit: unit(info),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three address codewords heard on a control channel, as the sigidwiki recording carries
    /// them.
    const ALOHA: u64 = 0x8000_0401_B800;
    const MAINTENANCE: u64 = 0x8A80_4C72_04B4;
    const REGISTRATION: u64 = 0x8E80_4C70_1000;

    #[test]
    fn an_aloha_invites_everyone_on_the_control_channel() {
        assert_eq!(
            Message::read(ALOHA),
            Message::Aloha {
                function: 0,
                invited: Address {
                    prefix: 0,
                    ident: 0
                },
                channel_low: 6,
            }
        );
        assert_eq!(Message::read(ALOHA).mnemonic(), "ALH");
    }

    #[test]
    fn broadcasts_name_the_system_they_describe() {
        assert_eq!(
            Message::read(MAINTENANCE),
            Message::Broadcast {
                definition: 2,
                system: 0x5009,
                channel: 0x204,
                adjacent_site: 4,
            }
        );
        assert_eq!(Message::read(MAINTENANCE).own_system(), Some(0x5009));
        let Message::Broadcast {
            definition, system, ..
        } = Message::read(REGISTRATION)
        else {
            panic!("a broadcast");
        };
        assert_eq!((definition, system), (3, 0x5009));
    }

    #[test]
    fn a_grant_names_both_parties_and_the_channel() {
        let info = 1 << 47 | 10 << 40 | 4105 << 27 | 123 << 15 | 77 << 2;
        assert_eq!(
            Message::read(info),
            Message::Gtc {
                called: Address {
                    prefix: 10,
                    ident: 4105
                },
                calling: Address {
                    prefix: 10,
                    ident: 77
                },
                channel: 123,
                data: false,
            }
        );
    }

    #[test]
    fn a_short_data_header_counts_its_codewords() {
        let info = 1 << 47 | 3 << 40 | 99 << 27 | 1 << 26 | 0b001 << 23 | 1 << 22 | 0b10 << 20;
        let message = Message::read(info);
        assert_eq!(message.mnemonic(), "HEAD");
        assert_eq!(message.appended(), 3);
    }

    #[test]
    fn an_address_reads_as_prefix_over_ident() {
        let address = Address {
            prefix: 10,
            ident: 4105,
        };
        assert_eq!(address.number(), 10 * 8192 + 4105);
        assert_eq!(address.label(), "10/4105");
    }
}
