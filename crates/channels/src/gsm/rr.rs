use sdrmm_wire::{GsmAssignment, GsmCcch, GsmCell, GsmChannelType, GsmHopping, GsmIdentity, GsmSi};

use super::{
    coding::BLOCK_BYTES,
    freq_list,
    identity::{plmn, sized_identity, tagged_identity, tmsi},
};

const RR_DISCRIMINATOR: u8 = 0x06;
const MOBILE_IDENTITY_TAG: u8 = 0x17;
const RXLEV_FLOOR_DBM: i16 = -110;
const T3212_UNIT_MIN: u16 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rr {
    SystemInformation(GsmSi),
    Paging(Vec<GsmIdentity>),
    Assignment(Vec<GsmAssignment>),
    Reject(Vec<u8>),
    Other(u8),
}

pub(crate) fn si_type(message_type: u8) -> Option<GsmSi> {
    Some(match message_type {
        0x19 => GsmSi::Si1,
        0x1A => GsmSi::Si2,
        0x02 => GsmSi::Si2bis,
        0x03 => GsmSi::Si2ter,
        0x07 => GsmSi::Si2quater,
        0x1B => GsmSi::Si3,
        0x1C => GsmSi::Si4,
        0x1D => GsmSi::Si5,
        0x05 => GsmSi::Si5bis,
        0x06 => GsmSi::Si5ter,
        0x1E => GsmSi::Si6,
        0x1F => GsmSi::Si7,
        0x18 => GsmSi::Si8,
        0x04 => GsmSi::Si9,
        0x00 => GsmSi::Si13,
        _ => return None,
    })
}

pub(crate) fn unit_data(block: &[u8; BLOCK_BYTES]) -> Option<(u8, &[u8])> {
    let [pseudo_length, discriminator, message_type, ..] = *block;
    if pseudo_length & 0x03 != 0x01 || discriminator != RR_DISCRIMINATOR {
        return None;
    }
    Some((message_type, &block[3..]))
}

pub(crate) fn parse(message_type: u8, body: &[u8]) -> Option<Rr> {
    if let Some(si) = si_type(message_type) {
        return Some(Rr::SystemInformation(si));
    }
    Some(match message_type {
        0x21 => Rr::Paging(paging_one(body)),
        0x22 => Rr::Paging(paging_two(body)),
        0x24 => Rr::Paging(paging_three(body)),
        0x3F => Rr::Assignment(immediate(body)?),
        0x39 => Rr::Assignment(immediate_extended(body)?),
        0x3A => Rr::Reject(
            [1, 5, 9, 13]
                .iter()
                .filter_map(|&at| body.get(at).copied())
                .collect(),
        ),
        other => Rr::Other(other),
    })
}

fn paging_one(body: &[u8]) -> Vec<GsmIdentity> {
    let mut identities = Vec::new();
    let Some(&first) = body.get(1) else {
        return identities;
    };
    identities.extend(sized_identity(&body[1..]));
    let next = 2 + usize::from(first);
    identities.extend(
        body.get(next..)
            .and_then(|rest| tagged_identity(rest, MOBILE_IDENTITY_TAG)),
    );
    identities
}

fn paging_two(body: &[u8]) -> Vec<GsmIdentity> {
    let mut identities: Vec<GsmIdentity> = [1..5, 5..9]
        .into_iter()
        .filter_map(|range| body.get(range).and_then(tmsi))
        .collect();
    identities.extend(
        body.get(9..)
            .and_then(|rest| tagged_identity(rest, MOBILE_IDENTITY_TAG)),
    );
    identities
}

fn paging_three(body: &[u8]) -> Vec<GsmIdentity> {
    [1..5, 5..9, 9..13, 13..17]
        .into_iter()
        .filter_map(|range| body.get(range).and_then(tmsi))
        .collect()
}

fn immediate(body: &[u8]) -> Option<Vec<GsmAssignment>> {
    let packet = body.first()? & 0x40 != 0;
    Some(vec![assignment(body.get(1..8)?, packet)?])
}

fn immediate_extended(body: &[u8]) -> Option<Vec<GsmAssignment>> {
    Some(vec![
        assignment(body.get(1..8)?, false)?,
        assignment(body.get(8..15)?, false)?,
    ])
}

fn assignment(octets: &[u8], packet: bool) -> Option<GsmAssignment> {
    let [kind, second, third, request, _, _, advance] = octets.try_into().ok()?;
    let code = kind >> 3;
    let (channel, subchannel) = match code {
        _ if packet => (GsmChannelType::Pdch, None),
        0b00001 => (GsmChannelType::TchF, None),
        0b00010..=0b00011 => (GsmChannelType::TchH, Some(code & 1)),
        0b00100..=0b00111 => (GsmChannelType::Sdcch4, Some(code & 3)),
        0b01000..=0b01111 => (GsmChannelType::Sdcch8, Some(code & 7)),
        _ => return None,
    };
    let hopping = second & 0x10 != 0;
    Some(GsmAssignment {
        channel,
        timeslot: kind & 7,
        subchannel,
        arfcn: (!hopping).then_some(u16::from(second & 3) << 8 | u16::from(third)),
        hopping: hopping.then_some(GsmHopping {
            maio: (second & 0x0F) << 2 | third >> 6,
            hsn: third & 0x3F,
        }),
        tsc: second >> 5,
        request,
        timing_advance: advance & 0x3F,
    })
}

#[derive(Debug, Default)]
pub(crate) struct CellBook {
    cell: GsmCell,
    neighbours: [Vec<u16>; 4],
}

impl CellBook {
    pub(crate) fn cell(&self) -> &GsmCell {
        &self.cell
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn apply(&mut self, si: GsmSi, body: &[u8]) {
        if !self.cell.system_information.contains(&si) {
            self.cell.system_information.push(si);
            self.cell.system_information.sort_by_key(|seen| *seen as u8);
        }
        match si {
            GsmSi::Si1 => {
                self.cell.cell_channels.clear();
                freq_list::decode(
                    body.get(..16).unwrap_or_default(),
                    &mut self.cell.cell_channels,
                );
                self.rach(body.get(16..19));
            }
            GsmSi::Si2 => {
                self.neighbours_from(0, body.get(..16));
                self.rach(body.get(17..20));
            }
            GsmSi::Si2bis => self.neighbours_from(1, body.get(..16)),
            GsmSi::Si2ter => self.neighbours_from(2, body.get(..16)),
            GsmSi::Si5 => self.neighbours_from(3, body.get(..16)),
            GsmSi::Si3 => self.system_information_3(body),
            GsmSi::Si4 => {
                self.location(body.get(..5));
                self.selection(body.get(5..7));
                self.rach(body.get(7..10));
            }
            GsmSi::Si6 => {
                self.cell_identity(body.get(..2));
                self.location(body.get(2..7));
            }
            GsmSi::Si13 => self.cell.gprs = true,
            _ => {}
        }
    }

    fn system_information_3(&mut self, body: &[u8]) {
        self.cell_identity(body.get(..2));
        self.location(body.get(2..7));
        if let Some(&[conf, _, t3212]) = body.get(7..10) {
            self.cell.ccch = ccch(conf & 7);
            self.cell.t3212_min = (t3212 != 0).then_some(u16::from(t3212) * T3212_UNIT_MIN);
        }
        self.selection(body.get(11..13));
        self.rach(body.get(13..16));
    }

    fn cell_identity(&mut self, octets: Option<&[u8]>) {
        if let Some(&[high, low]) = octets {
            self.cell.cell_id = Some(u16::from_be_bytes([high, low]));
        }
    }

    fn location(&mut self, octets: Option<&[u8]>) {
        let Some(octets) = octets.filter(|octets| octets.len() == 5) else {
            return;
        };
        if let Some((mcc, mnc)) = plmn(&octets[..3]) {
            self.cell.mcc = Some(mcc);
            self.cell.mnc = Some(mnc);
        }
        self.cell.lac = Some(u16::from_be_bytes([octets[3], octets[4]]));
    }

    fn selection(&mut self, octets: Option<&[u8]>) {
        if let Some(&[power, access]) = octets {
            self.cell.ms_txpwr_max_cch = Some(power & 0x1F);
            self.cell.rxlev_access_min_dbm = Some(RXLEV_FLOOR_DBM + i16::from(access & 0x3F));
        }
    }

    fn rach(&mut self, octets: Option<&[u8]>) {
        if let Some(&[first, _, _]) = octets {
            self.cell.barred = first & 0x02 != 0;
        }
    }

    fn neighbours_from(&mut self, source: usize, octets: Option<&[u8]>) {
        let set = &mut self.neighbours[source];
        set.clear();
        if let Some(octets) = octets {
            freq_list::decode(octets, set);
        }
        let mut all: Vec<u16> = self.neighbours.iter().flatten().copied().collect();
        all.sort_unstable();
        all.dedup();
        self.cell.neighbours = all;
    }

    pub(crate) fn combined(&self) -> bool {
        self.cell.ccch == Some(GsmCcch::Combined)
    }
}

fn ccch(conf: u8) -> Option<GsmCcch> {
    Some(match conf {
        0 => GsmCcch::Separate,
        1 => GsmCcch::Combined,
        2 => GsmCcch::Two,
        4 => GsmCcch::Three,
        6 => GsmCcch::Four,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::GsmIdentityKind;

    use super::*;

    fn block(hex: &str) -> [u8; BLOCK_BYTES] {
        let mut out = [0u8; BLOCK_BYTES];
        for (k, octet) in out.iter_mut().enumerate() {
            *octet = u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).unwrap();
        }
        out
    }

    fn rr(hex: &str) -> (Option<Rr>, [u8; BLOCK_BYTES]) {
        let block = block(hex);
        let parsed = unit_data(&block).and_then(|(kind, body)| parse(kind, body));
        (parsed, block)
    }

    #[test]
    fn system_information_3_names_the_cell() {
        let (parsed, block) = rr("49061b2bd962f220013ec8070a156009b90000e81f461b");
        assert_eq!(parsed, Some(Rr::SystemInformation(GsmSi::Si3)));
        let mut book = CellBook::default();
        book.apply(GsmSi::Si3, &block[3..]);
        let cell = book.cell();
        assert_eq!(cell.cgi().as_deref(), Some("262-02-318-11225"));
        assert_eq!(cell.ccch, Some(GsmCcch::Separate));
        assert_eq!(cell.t3212_min, Some(60));
        assert_eq!(cell.rxlev_access_min_dbm, Some(-101));
        assert!(!cell.barred);
    }

    #[test]
    fn system_information_1_and_2_list_channels() {
        let mut book = CellBook::default();
        let (_, si1) = rr("5506198f6a8000000000000000000000000000b9000083");
        book.apply(GsmSi::Si1, &si1[3..]);
        let (_, si2) = rr("59061a8f6d181080000000000000000000000078b90000");
        book.apply(GsmSi::Si2, &si2[3..]);
        assert_eq!(book.cell().cell_channels, [725]);
        assert_eq!(book.cell().neighbours, [730, 733, 734, 741, 746]);
        assert_eq!(book.cell().system_information, [GsmSi::Si1, GsmSi::Si2]);
    }

    #[test]
    fn paging_requests_carry_their_identities() {
        let (parsed, _) = rr("310621200829435109280546128b2b2b2b2b2b2b2b2b2b");
        let Some(Rr::Paging(identities)) = parsed else {
            panic!("paging");
        };
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].kind, GsmIdentityKind::Imsi);
        let (parsed, _) = rr("410621a005f40156da8d1705f40156e4988b2b2b2b2b2b");
        let Some(Rr::Paging(identities)) = parsed else {
            panic!("paging");
        };
        let labels: Vec<String> = identities.iter().map(GsmIdentity::label).collect();
        assert_eq!(labels, ["TMSI 0156DA8D", "TMSI 0156E498"]);
        let (parsed, _) = rr("1506210001f08b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b");
        assert_eq!(parsed, Some(Rr::Paging(Vec::new())));
    }

    #[test]
    fn an_immediate_assignment_names_the_dedicated_channel() {
        let (parsed, _) = rr("2d063f004902d52654c701002b2b2b2b2b2b2b2b2b2b2b");
        let Some(Rr::Assignment(assignments)) = parsed else {
            panic!("assignment");
        };
        assert_eq!(
            assignments,
            [GsmAssignment {
                channel: GsmChannelType::Sdcch8,
                timeslot: 1,
                subchannel: Some(1),
                arfcn: Some(725),
                hopping: None,
                tsc: 0,
                request: 0x26,
                timing_advance: 1,
            }]
        );
    }

    #[test]
    fn fill_frames_are_not_rr_messages() {
        assert_eq!(rr("032b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b").0, None);
    }
}
