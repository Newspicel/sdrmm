use sdrmm_wire::{DvFrame, DvFrameKind, DvMode};

use super::{
    cmce::{self, Cmce, Kind},
    llc::{self, Discriminator},
    mac::Address,
    sds::Payload,
};

const USAGE_MARKERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) timeslot: u8,
    pub(crate) colour: u8,
    pub(crate) mcc: u16,
    pub(crate) mnc: u16,
    pub(crate) location_area: Option<u16>,
}

impl Stamp {
    pub(crate) fn frame(self, kind: DvFrameKind) -> DvFrame {
        DvFrame {
            slot: Some(self.timeslot),
            color_code: Some(u16::from(self.colour)),
            system_id: Some(self.mcc),
            network_id: Some(u32::from(self.mnc)),
            site_id: self.location_area,
            crc_verified: Some(true),
            ..DvFrame::new(DvMode::Tetra, kind)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Call {
    id: u16,
    destination: Option<u32>,
    source: Option<u32>,
    group: Option<bool>,
    encrypted: Option<bool>,
}

pub(crate) struct Upper {
    calls: [Option<Call>; USAGE_MARKERS],
}

impl Upper {
    pub(crate) fn new() -> Self {
        Self {
            calls: [None; USAGE_MARKERS],
        }
    }

    pub(crate) fn reset(&mut self) {
        self.calls = [None; USAGE_MARKERS];
    }

    pub(crate) fn traffic(&self, stamp: Stamp, marker: u8, out: &mut Vec<DvFrame>) {
        let mut frame = stamp.frame(DvFrameKind::Voice);
        if let Some(call) = self.calls.get(usize::from(marker)).copied().flatten() {
            describe(&mut frame, &call);
        }
        out.push(frame);
    }

    pub(crate) fn sdu(
        &mut self,
        stamp: Stamp,
        address: Address,
        bits: &[bool],
        out: &mut Vec<DvFrame>,
    ) {
        let Some((discriminator, body)) = llc::tl_sdu(bits).and_then(llc::discriminator) else {
            return;
        };
        match discriminator {
            Discriminator::Cmce => {
                if let Some(pdu) = cmce::parse(body) {
                    self.cmce(stamp, address, pdu, out);
                }
            }
            Discriminator::Mm => {
                if let Some(name) = cmce::mm_name(body) {
                    let mut frame = stamp.frame(DvFrameKind::Control);
                    frame.opcode = Some(name.to_owned());
                    frame.destination = address.ssi();
                    frame.group_call = Some(false);
                    out.push(frame);
                }
            }
            Discriminator::Sndcp | Discriminator::Mle | Discriminator::Other => {}
        }
    }

    fn cmce(&mut self, stamp: Stamp, address: Address, pdu: Cmce, out: &mut Vec<DvFrame>) {
        let known = pdu.call_id.and_then(|id| self.find(id));
        let call = self.track(address, &pdu, known);
        let kind = match pdu.kind {
            Kind::Setup => DvFrameKind::Header,
            Kind::Release => DvFrameKind::Terminator,
            Kind::Status | Kind::ShortData => DvFrameKind::Data,
            Kind::Connect | Kind::Granted | Kind::Ceased | Kind::Other => DvFrameKind::Control,
        };
        let mut frame = stamp.frame(kind);
        frame.opcode = Some(pdu.name.to_owned());
        match call {
            Some(call) => describe(&mut frame, &call),
            None => {
                frame.destination = address.ssi();
                frame.source = pdu.party;
                frame.group_call = pdu.group;
                frame.encrypted = pdu.encrypted;
            }
        }
        if let Some(payload) = pdu.payload {
            carry(&mut frame, payload);
        }
        out.push(frame);
    }

    fn find(&self, id: u16) -> Option<usize> {
        self.calls
            .iter()
            .position(|call| call.is_some_and(|call| call.id == id))
    }

    fn track(&mut self, address: Address, pdu: &Cmce, known: Option<usize>) -> Option<Call> {
        let id = pdu.call_id?;
        if pdu.kind == Kind::Release {
            let call = known.and_then(|index| self.calls[index].take());
            return call;
        }
        let index = address.usage_marker().map(usize::from).or(known)?;
        let previous = known
            .and_then(|index| self.calls[index])
            .filter(|call| call.id == id);
        if let Some(old) = known.filter(|&old| old != index) {
            self.calls[old] = None;
        }
        let mut call = previous.unwrap_or(Call {
            id,
            ..Call::default()
        });
        if pdu.kind == Kind::Setup {
            call.destination = address.ssi();
        }
        let addressee = address.ssi().filter(|&ssi| call.destination != Some(ssi));
        let granted_to = (pdu.kind == Kind::Granted).then_some(addressee).flatten();
        call.source = pdu.party.or(granted_to).or(call.source);
        call.group = pdu.group.or(call.group);
        call.encrypted = pdu.encrypted.or(call.encrypted);
        let slot = self.calls.get_mut(index)?;
        *slot = Some(call);
        Some(call)
    }
}

fn describe(frame: &mut DvFrame, call: &Call) {
    frame.destination = call.destination;
    frame.source = call.source;
    frame.group_call = call.group;
    frame.encrypted = call.encrypted;
}

fn carry(frame: &mut DvFrame, payload: Payload) {
    match payload {
        Payload::Text(text) => frame.text = Some(text),
        Payload::Position { lat, lon, error_m } => {
            frame.lat = Some(lat);
            frame.lon = Some(lon);
            frame.position_error_m = error_m;
        }
        Payload::Status(status) => frame.data = Some(format!("status {status:04X}")),
        Payload::Data { protocol, hex } => {
            frame.data = Some(match protocol {
                Some(protocol) => format!("{protocol:02X}: {hex}"),
                None => hex,
            });
        }
    }
}
