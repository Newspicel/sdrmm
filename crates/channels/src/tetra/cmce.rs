use super::{
    bits::Reader,
    sds::{self, Payload},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Setup,
    Connect,
    Granted,
    Ceased,
    Release,
    Status,
    ShortData,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cmce {
    pub(crate) kind: Kind,
    pub(crate) name: &'static str,
    pub(crate) call_id: Option<u16>,
    pub(crate) group: Option<bool>,
    pub(crate) encrypted: Option<bool>,
    pub(crate) party: Option<u32>,
    pub(crate) cause: Option<u8>,
    pub(crate) payload: Option<Payload>,
}

const NAMES: [&str; 17] = [
    "D-ALERT",
    "D-CALL PROCEEDING",
    "D-CONNECT",
    "D-CONNECT ACK",
    "D-DISCONNECT",
    "D-INFO",
    "D-RELEASE",
    "D-SETUP",
    "D-STATUS",
    "D-TX CEASED",
    "D-TX CONTINUE",
    "D-TX GRANTED",
    "D-TX WAIT",
    "D-TX INTERRUPT",
    "D-CALL RESTORE",
    "D-SDS DATA",
    "D-FACILITY",
];

const CONNECT: usize = 2;
const RELEASE: usize = 6;
const SETUP: usize = 7;
const STATUS: usize = 8;
const TX_CEASED: usize = 9;
const TX_GRANTED: usize = 11;
const SDS_DATA: usize = 15;
const FACILITY: usize = 16;

const CALL_ID_BITS: usize = 14;
const NOTIFICATION_BITS: usize = 6;
const TEMPORARY_ADDRESS_BITS: usize = 24;

pub(crate) fn parse(bits: &[bool]) -> Option<Cmce> {
    let mut r = Reader::new(bits);
    let pdu = usize::try_from(r.take(5)?).ok()?;
    let name = *NAMES.get(pdu)?;
    let mut cmce = Cmce {
        kind: Kind::Other,
        name,
        call_id: None,
        group: None,
        encrypted: None,
        party: None,
        cause: None,
        payload: None,
    };
    match pdu {
        SETUP => setup(&mut r, &mut cmce),
        CONNECT => connect(&mut r, &mut cmce),
        TX_GRANTED => granted(&mut r, &mut cmce),
        TX_CEASED => {
            cmce.kind = Kind::Ceased;
            cmce.call_id = Some(r.take_u16(CALL_ID_BITS)?);
            Some(())
        }
        RELEASE => {
            cmce.kind = Kind::Release;
            cmce.call_id = Some(r.take_u16(CALL_ID_BITS)?);
            cmce.cause = Some(r.take_u8(5)?);
            Some(())
        }
        STATUS => status(&mut r, &mut cmce),
        SDS_DATA => short_data(&mut r, &mut cmce),
        FACILITY => Some(()),
        _ => {
            cmce.call_id = Some(r.take_u16(CALL_ID_BITS)?);
            Some(())
        }
    }?;
    Some(cmce)
}

fn basic_service(r: &mut Reader<'_>, cmce: &mut Cmce) -> Option<()> {
    r.skip(3)?;
    cmce.encrypted = Some(r.flag()?);
    cmce.group = Some(r.take(2)? != 0);
    r.skip(2)
}

fn setup(r: &mut Reader<'_>, cmce: &mut Cmce) -> Option<()> {
    cmce.kind = Kind::Setup;
    cmce.call_id = Some(r.take_u16(CALL_ID_BITS)?);
    r.skip(4 + 1 + 1)?;
    basic_service(r, cmce)?;
    r.skip(2 + 1 + 4)?;
    if r.flag() != Some(true) {
        return Some(());
    }
    r.optional(NOTIFICATION_BITS)?;
    r.optional(TEMPORARY_ADDRESS_BITS)?;
    if r.flag()? {
        cmce.party = party(r)?;
    }
    Some(())
}

fn connect(r: &mut Reader<'_>, cmce: &mut Cmce) -> Option<()> {
    cmce.kind = Kind::Connect;
    cmce.call_id = Some(r.take_u16(CALL_ID_BITS)?);
    r.skip(4 + 1 + 1 + 2 + 1 + 1)?;
    if r.flag() != Some(true) {
        return Some(());
    }
    r.optional(4)?;
    if r.flag()? {
        basic_service(r, cmce)?;
    }
    Some(())
}

fn granted(r: &mut Reader<'_>, cmce: &mut Cmce) -> Option<()> {
    cmce.kind = Kind::Granted;
    cmce.call_id = Some(r.take_u16(CALL_ID_BITS)?);
    r.skip(2 + 1)?;
    cmce.encrypted = Some(r.flag()?);
    r.skip(1)?;
    if r.flag() != Some(true) {
        return Some(());
    }
    r.optional(NOTIFICATION_BITS)?;
    if r.flag()? {
        cmce.party = party(r)?;
    }
    Some(())
}

fn status(r: &mut Reader<'_>, cmce: &mut Cmce) -> Option<()> {
    cmce.kind = Kind::Status;
    cmce.party = party(r)?;
    cmce.payload = Some(Payload::Status(r.take_u16(16)?));
    Some(())
}

fn short_data(r: &mut Reader<'_>, cmce: &mut Cmce) -> Option<()> {
    cmce.kind = Kind::ShortData;
    cmce.party = party(r)?;
    cmce.payload = Some(sds::parse(r)?);
    Some(())
}

fn party(r: &mut Reader<'_>) -> Option<Option<u32>> {
    match r.take(2)? {
        0 => {
            r.skip(8)?;
            Some(None)
        }
        1 => Some(Some(r.take(24)?)),
        2 => {
            let ssi = r.take(24)?;
            r.skip(24)?;
            Some(Some(ssi))
        }
        _ => None,
    }
}

const MM_NAMES: [Option<&str>; 16] = [
    Some("D-OTAR"),
    Some("D-AUTHENTICATION"),
    Some("D-CK CHANGE DEMAND"),
    Some("D-DISABLE"),
    Some("D-ENABLE"),
    Some("D-LOCATION UPDATE ACCEPT"),
    Some("D-LOCATION UPDATE COMMAND"),
    Some("D-LOCATION UPDATE REJECT"),
    None,
    Some("D-LOCATION UPDATE PROCEEDING"),
    Some("D-ATTACH/DETACH GROUP ID"),
    Some("D-ATTACH/DETACH GROUP ID ACK"),
    Some("D-MM STATUS"),
    None,
    None,
    Some("MM PDU NOT SUPPORTED"),
];

pub(crate) fn mm_name(bits: &[bool]) -> Option<&'static str> {
    let index = usize::try_from(Reader::new(bits).take(4)?).ok()?;
    MM_NAMES[index]
}
