use sdrmm_wire::{
    TetrapolAddress, TetrapolAddressKind, TetrapolCallType, TetrapolCell, TetrapolGroupCall,
    TetrapolKey, TetrapolPayload, TetrapolSystemInfo,
};

const SYSTEM_INFO: u8 = 0x90;
const GROUP_ACTIVATION: u8 = 0x55;
const GROUP_END: u8 = 0x57;
const GROUP_IDLE: u8 = 0x58;
const GROUP_PAGING: u8 = 0x5B;
const BSC_DISCONNECTED: u8 = 0b100;
const SYSTEM_INFO_STABLE: usize = 13;
pub(crate) const BCH_BLOCK_BIT: u8 = 0x10;

const NAMES: [(u8, &str); 76] = [
    (0x08, "reject"),
    (0x09, "refusal"),
    (0x0A, "end"),
    (0x0B, "back to control channel"),
    (0x0C, "release"),
    (0x0F, "hook on invitation"),
    (0x10, "return"),
    (0x12, "call waiting"),
    (0x13, "authentication"),
    (0x16, "authorisation"),
    (0x18, "channel init"),
    (0x21, "registration refused"),
    (0x22, "registration accepted"),
    (0x23, "forced registration"),
    (0x25, "location activity ack"),
    (0x28, "periodic access subscription"),
    (0x29, "periodic access subscription"),
    (0x31, "call alert"),
    (0x32, "call setup"),
    (0x34, "call connect"),
    (0x35, "call switch"),
    (0x39, "transfer refused"),
    (0x3E, "call start"),
    (0x42, "functional short data"),
    (0x45, "data message"),
    (0x46, "explicit short data"),
    (0x48, "data end"),
    (0x49, "datagram notify"),
    (0x4A, "datagram"),
    (0x4B, "broadcast"),
    (0x4C, "data service"),
    (0x4E, "data down status"),
    (0x53, "emergency notification"),
    (0x55, "group activation"),
    (0x56, "emergency channel activation"),
    (0x57, "group end"),
    (0x58, "group idle"),
    (0x59, "group reject"),
    (0x5A, "emergency channel reject"),
    (0x5B, "group paging"),
    (0x5C, "broadcast notification"),
    (0x5D, "crisis notification"),
    (0x5F, "emergency ack"),
    (0x60, "connect data channel"),
    (0x62, "connect control channel"),
    (0x63, "data authentication"),
    (0x64, "data request"),
    (0x65, "data channel open"),
    (0x67, "extended status"),
    (0x68, "control channel open"),
    (0x69, "broadcast waiting"),
    (0x70, "access disabled"),
    (0x71, "traffic enabled"),
    (0x72, "traffic disabled"),
    (0x76, "deviation on"),
    (0x77, "ability management"),
    (0x78, "service disabled"),
    (0x80, "emergency refused"),
    (0x82, "group overload"),
    (0x83, "emergency channel overload"),
    (0x84, "priority group waiting"),
    (0x85, "priority group activation"),
    (0x86, "open channel activation"),
    (0x87, "open channel reject"),
    (0x88, "open channel paging"),
    (0x90, "system info"),
    (0x92, "group list"),
    (0x93, "group composition"),
    (0x94, "neighbouring cells"),
    (0x95, "emergency channel description"),
    (0x96, "additional participants"),
    (0x97, "data channel description"),
    (0x98, "talkgroup priority list"),
    (0x99, "group master"),
    (0xC5, "information delivery"),
    (0xE2, "call end"),
];

pub(crate) fn name(codop: u8) -> String {
    NAMES.iter().find(|(value, _)| *value == codop).map_or_else(
        || format!("codop {codop:#04x}"),
        |(_, name)| (*name).to_owned(),
    )
}

pub(crate) fn address(high: u8, low: u8) -> Option<TetrapolAddress> {
    let z = high >> 7;
    let y = high >> 4 & 0b111;
    let x = u16::from(high & 0x0F) << 8 | u16::from(low);
    let (kind, id) = match (z, y, x) {
        (0, 0b111, 0) => return None,
        (0, 0b111, 0x0FFF) => (TetrapolAddressKind::AllStations, 0x7FFF),
        (1, _, _) => (TetrapolAddressKind::Random, x),
        (_, 0, _) => (TetrapolAddressKind::Group, x),
        (_, 1, _) => (TetrapolAddressKind::Object, x),
        _ => (TetrapolAddressKind::Terminal, u16::from(y) << 12 | x),
    };
    Some(TetrapolAddress { kind, id })
}

pub(crate) fn stable_len(tsdu: &[u8]) -> usize {
    match tsdu {
        [SYSTEM_INFO, state, ..] if state >> 5 != BSC_DISCONNECTED => {
            tsdu.len().min(SYSTEM_INFO_STABLE)
        }
        _ => tsdu.len(),
    }
}

pub(crate) fn payload(tsdu: &[u8]) -> Option<TetrapolPayload> {
    let &codop = tsdu.first()?;
    let parsed = match codop {
        SYSTEM_INFO => system_info(tsdu).map(TetrapolPayload::SystemInfo),
        GROUP_ACTIVATION => group_activation(tsdu).map(TetrapolPayload::GroupActivation),
        GROUP_PAGING => group_paging(tsdu).map(TetrapolPayload::GroupPaging),
        GROUP_IDLE => tsdu
            .get(1)
            .map(|&cause| TetrapolPayload::GroupIdle { cause }),
        GROUP_END => tsdu
            .get(1)
            .map(|&cause| TetrapolPayload::GroupEnd { cause }),
        _ => None,
    };
    Some(parsed.unwrap_or_else(|| TetrapolPayload::Other {
        codop,
        name: name(codop),
    }))
}

fn right_aligned(high: u8, low: u8) -> u16 {
    u16::from(high & 0x0F) << 8 | u16::from(low)
}

fn call_type(mode: u8) -> TetrapolCallType {
    match mode & 0b11 {
        0b00 => TetrapolCallType::OpenChannel,
        0b01 => TetrapolCallType::Internal,
        0b10 => TetrapolCallType::Broadcast,
        _ => TetrapolCallType::External,
    }
}

fn key(octet: u8) -> TetrapolKey {
    TetrapolKey {
        kind: octet >> 4,
        index: octet & 0x0F,
    }
}

fn group_activation(tsdu: &[u8]) -> Option<TetrapolGroupCall> {
    let [
        _,
        mode_group,
        group,
        coverage,
        channel_high,
        channel,
        uplink,
        downlink,
        key_ref,
        ..,
    ] = *tsdu
    else {
        return None;
    };
    Some(TetrapolGroupCall {
        group: right_aligned(mode_group, group),
        call_type: call_type(mode_group >> 4),
        coverage,
        key: key(key_ref),
        channel: Some(right_aligned(channel_high, channel)),
        downlink_scrambling: Some(downlink & 0x7F),
        uplink_scrambling: Some(uplink & 0x7F),
    })
}

fn group_paging(tsdu: &[u8]) -> Option<TetrapolGroupCall> {
    let [_, mode_group, group, coverage, key_ref, ..] = *tsdu else {
        return None;
    };
    Some(TetrapolGroupCall {
        group: right_aligned(mode_group, group),
        call_type: call_type(mode_group >> 4),
        coverage,
        key: key(key_ref),
        channel: None,
        downlink_scrambling: None,
        uplink_scrambling: None,
    })
}

fn cell(bn: u8, first: u8, second: u8) -> TetrapolCell {
    let field1 = first & 0x3F;
    let field2 = second >> 4;
    let (rsw, bs) = if first >> 6 == 0b01 {
        (field1, field2)
    } else {
        (field2, field1)
    };
    TetrapolCell { bn, rsw, bs }
}

fn system_info(tsdu: &[u8]) -> Option<TetrapolSystemInfo> {
    let &state = tsdu.get(1)?;
    let mode = state >> 5;
    if mode == BSC_DISCONNECTED {
        return disconnected_system_info(tsdu, state);
    }
    let [
        _,
        _,
        _,
        country,
        system,
        location_area,
        bn,
        cell_high,
        cell_low,
        _,
        uplink,
        _,
        _,
        _,
        _,
        superframe_high,
        superframe,
        ..,
    ] = *tsdu
    else {
        return None;
    };
    Some(TetrapolSystemInfo {
        cell: cell(bn, cell_high, cell_low),
        country: Some(country),
        network: Some(system >> 4),
        location_area: Some(location_area),
        uplink_scrambling: uplink & 0x7F,
        mode,
        ciphered: state >> 3 & 1 == 0,
        home_only: state >> 1 & 1 == 1,
        superframe: Some(right_aligned(superframe_high, superframe)),
        control_channel: None,
    })
}

fn disconnected_system_info(tsdu: &[u8], state: u8) -> Option<TetrapolSystemInfo> {
    let [_, _, cell_low, bn, uplink, _, _, channel_high, channel, ..] = *tsdu else {
        return None;
    };
    Some(TetrapolSystemInfo {
        cell: cell(bn, cell_low, state << 4),
        country: None,
        network: None,
        location_area: None,
        uplink_scrambling: uplink & 0x7F,
        mode: BSC_DISCONNECTED,
        ciphered: false,
        home_only: false,
        superframe: None,
        control_channel: Some(right_aligned(channel_high, channel)),
    })
}

pub(crate) fn first_bch_block(tsdu: &[u8]) -> Option<bool> {
    match tsdu {
        [SYSTEM_INFO, state, ..] if state >> 5 != BSC_DISCONNECTED => {
            Some(state & BCH_BLOCK_BIT == 0)
        }
        _ => None,
    }
}
