//! The 28-bit outbound control channel messages, after TIA/EIA TSB-69.3 for standard and
//! networked sites and as DSD-FME reverse engineered them for extended addressing.

use sdrmm_wire::DvFrame;

use super::trunk;

/// The extended site key, laid over the top byte of every message on an ESK system.
const ESK_MASK: u32 = 0xA0 << 20;
/// The first logical channel number that reports a call's state rather than a channel.
const FIRST_STATUS_LCN: u8 = 26;
const STATUS_REQUEST: u8 = 248;

/// What one control channel message says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Message {
    GroupCall(Call),
    IndividualCall(Call),
    AllCall(Call),
    DataCall { target: u32, group: bool, lcn: u8 },
    Interconnect { target: u32, group: bool, lcn: u8 },
    Login { group: u32, unit: u32 },
    Status { unit: u32, status: u8 },
    Site { site: u16, control_lcn: u8 },
    ExtendedSite { site: u16, area: u8 },
    System { system: u16, control_lcn: u8 },
    AdjacentSite { site: u16, lcn: u8 },
    Console { unit: u32, lcn: u8, drop: bool },
    TestCall { lcn: u8 },
    Other(&'static str),
}

/// A voice call a channel was assigned to, or is still held on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Call {
    pub(super) target: u32,
    pub(super) source: Option<u32>,
    pub(super) lcn: u8,
    pub(super) digital: bool,
    pub(super) emergency: bool,
    pub(super) update: bool,
}

/// One message or two: a frame's second message is either its own or the first one's tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Read {
    pub(super) first: Message,
    pub(super) second: Option<Message>,
}

const fn field(word: u32, shift: u32, bits: u32) -> u32 {
    word >> shift & ((1 << bits) - 1)
}

/// Reads the messages of a frame whose first word was read; `second` is `None` when the second
/// could not be. An extended message spans both words, so it needs both.
pub(super) fn read(first: u32, second: Option<u32>, extended: bool, esk: bool) -> Option<Read> {
    let unkey = |word: u32| if esk { word ^ ESK_MASK } else { word };
    let first = unkey(first);
    let second = second.map(unkey);
    if extended {
        return Some(Read {
            first: extended_message(first, second?),
            second: None,
        });
    }
    let (message, paired) = standard(first, second);
    let second = second
        .filter(|&word| !paired && word != first)
        .map(|word| standard(word, None).0);
    Some(Read {
        first: message,
        second,
    })
}

/// A standard message, and whether it took the frame's second word as its own tail.
fn standard(word: u32, tail: Option<u32>) -> (Message, bool) {
    match field(word, 25, 3) {
        0..=3 => (voice_group_assignment(word, tail), true),
        5 => (data_call(word), false),
        6 => (
            Message::Login {
                group: field(word, 14, 11),
                unit: field(word, 0, 14),
            },
            false,
        ),
        7 => standard_b(word, tail),
        _ => (Message::Other("Reserved"), false),
    }
}

fn voice_group_assignment(word: u32, tail: Option<u32>) -> Message {
    let kind = field(word, 25, 3);
    Message::GroupCall(Call {
        target: field(word, 0, 11),
        source: tail.map(|tail| field(word, 18, 7) << 7 | field(tail, 17, 7)),
        lcn: field(word, 12, 5) as u8,
        digital: kind >= 2,
        emergency: kind & 1 == 1,
        update: false,
    })
}

fn data_call(word: u32) -> Message {
    let group = field(word, 14, 1) == 0;
    Message::DataCall {
        target: field(word, 0, if group { 11 } else { 14 }),
        group,
        lcn: field(word, 15, 5) as u8,
    }
}

fn standard_b(word: u32, tail: Option<u32>) -> (Message, bool) {
    let lcn = field(word, 15, 5) as u8;
    match field(word, 22, 3) {
        0 => (status(field(word, 0, 14), field(word, 14, 8) as u8), false),
        1 => {
            let group = field(word, 14, 1) == 0;
            (
                Message::Interconnect {
                    target: field(word, 0, if group { 11 } else { 14 }),
                    group,
                    lcn,
                },
                false,
            )
        }
        3 => channel_update(word, tail),
        4 => (Message::Other("System assigned ID"), false),
        5 => (individual_assignment(word, tail), true),
        6 => (
            Message::Console {
                unit: field(word, 0, 14),
                lcn: field(word, 14, 5) as u8,
                drop: field(word, 19, 1) == 1,
            },
            false,
        ),
        7 => standard_d(word, tail),
        _ => (Message::Other("Reserved"), false),
    }
}

fn status(unit: u32, status: u8) -> Message {
    if status == STATUS_REQUEST {
        Message::Other("Status request")
    } else {
        Message::Status { unit, status }
    }
}

/// A group update stands alone; an individual one carries its caller in the second word.
fn channel_update(word: u32, tail: Option<u32>) -> (Message, bool) {
    let individual = field(word, 14, 1) == 1;
    let kind = field(word, 20, 2);
    let mut call = Call {
        lcn: field(word, 15, 5) as u8,
        digital: kind & 1 == 1,
        update: true,
        ..Call::default()
    };
    if individual {
        call.target = field(word, 0, 14);
        call.source = tail.map(|tail| field(tail, 0, 14));
        if call.target == 0 && call.source == Some(0) {
            return (Message::TestCall { lcn: call.lcn }, true);
        }
        return (Message::IndividualCall(call), true);
    }
    call.target = field(word, 0, 11);
    call.emergency = field(word, 13, 1) == 1;
    (Message::GroupCall(call), false)
}

fn individual_assignment(word: u32, tail: Option<u32>) -> Message {
    let call = Call {
        target: field(word, 0, 14),
        source: tail.map(|tail| field(tail, 0, 14)),
        lcn: field(word, 15, 5) as u8,
        digital: field(word, 14, 1) == 1,
        ..Call::default()
    };
    if call.target == 0 && call.source == Some(0) {
        Message::TestCall { lcn: call.lcn }
    } else {
        Message::IndividualCall(call)
    }
}

fn standard_d(word: u32, tail: Option<u32>) -> (Message, bool) {
    let lcn = field(word, 12, 5) as u8;
    let message = match field(word, 17, 5) {
        0x00 => Message::Other("Cancel regroup"),
        0x01 => Message::AdjacentSite {
            site: field(word, 4, 5) as u16,
            lcn,
        },
        0x02 => Message::Other("Site options"),
        0x04 => Message::Other("Regroup plan"),
        0x05 => Message::Other("Auxiliary control channel"),
        0x06 => Message::TestCall {
            lcn: field(word, 7, 5) as u8,
        },
        0x07 => Message::Other("Unit enable"),
        0x08..=0x0B => Message::Site {
            site: field(word, 0, 5) as u16,
            control_lcn: lcn,
        },
        0x0F => {
            return (
                Message::AllCall(Call {
                    source: tail.map(|tail| field(word, 0, 7) | field(tail, 1, 7) << 7),
                    lcn,
                    digital: field(word, 11, 1) == 1,
                    update: field(word, 10, 1) == 1,
                    ..Call::default()
                }),
                true,
            );
        }
        0x10 => return (Message::Other("Dynamic regroup"), true),
        _ => Message::Other("Reserved"),
    };
    (message, false)
}

fn extended_message(first: u32, second: u32) -> Message {
    let source = Some(field(second, 0, 20));
    let lcn = field(first, 17, 5) as u8;
    match field(first, 23, 5) {
        0x1F => extended_system(first, second),
        0x01 => Message::Other("TDMA group call"),
        0x02 => Message::DataCall {
            target: field(first, 0, 16),
            group: true,
            lcn,
        },
        kind @ (0x03 | 0x06) => Message::GroupCall(Call {
            target: field(first, 0, 16),
            source,
            lcn,
            digital: kind == 0x03,
            emergency: field(second, 20, 1) == 1,
            update: field(first, 16, 1) == 1,
        }),
        0x10 => extended_individual(first, second),
        0x12 => Message::Other("Data channel assignment"),
        0x16 => Message::AllCall(Call {
            source,
            lcn,
            digital: field(first, 16, 1) == 1,
            update: field(first, 15, 1) == 1,
            ..Call::default()
        }),
        0x19 => Message::Login {
            group: field(first, 0, 16),
            unit: field(second, 0, 20),
        },
        _ => Message::Other("Reserved"),
    }
}

fn extended_individual(first: u32, second: u32) -> Message {
    let call = Call {
        target: field(first, 0, 20),
        source: Some(field(second, 0, 20)),
        lcn: field(second, 20, 5) as u8,
        digital: field(first, 21, 1) == 1,
        update: field(first, 20, 1) == 1,
        ..Call::default()
    };
    if call.target == 0 && call.source == Some(0) {
        Message::TestCall { lcn: call.lcn }
    } else {
        Message::IndividualCall(call)
    }
}

fn extended_system(first: u32, second: u32) -> Message {
    match field(first, 19, 4) {
        0x0 => Message::TestCall {
            lcn: field(first, 7, 5) as u8,
        },
        0x1 => Message::AdjacentSite {
            site: field(first, 0, 8) as u16,
            lcn: field(first, 12, 5) as u8,
        },
        0x4 => status(field(second, 0, 20), field(first, 0, 8) as u8),
        0x7 => Message::Other("Unit enable"),
        0x8 => Message::System {
            system: field(first, 0, 16) as u16,
            control_lcn: field(second, 0, 5) as u8,
        },
        0xA => Message::ExtendedSite {
            site: (field(first, 12, 3) << 5 | field(first, 0, 5)) as u16,
            area: field(first, 5, 7) as u8,
        },
        0xB => Message::Other("Regroup plan"),
        0xC => Message::Other("Patch"),
        0xD => Message::Other("Serial number request"),
        _ => Message::Other("Reserved"),
    }
}

/// A logical channel number as a channel, or what the site says about the call instead.
pub(super) fn channel(lcn: u8) -> Result<u16, &'static str> {
    match lcn {
        0 => Err("no channel"),
        1..FIRST_STATUS_LCN => Ok(u16::from(lcn)),
        28 => Err("convert to callee"),
        29 => Err("queued"),
        30 => Err("system busy"),
        31 => Err("denied"),
        _ => Err("reserved"),
    }
}

impl Message {
    /// The message as the control channel burst the trunk follower and the event log read.
    pub(super) fn frame(&self) -> DvFrame {
        let mut frame = trunk::control_frame();
        match *self {
            Self::GroupCall(call) => call_frame(&mut frame, call, Some(true), "Group call"),
            Self::IndividualCall(call) => {
                call_frame(&mut frame, call, Some(false), "Individual call");
            }
            Self::AllCall(call) => call_frame(&mut frame, call, None, "All call"),
            Self::DataCall { target, group, lcn } => {
                frame.opcode = Some("Data call".to_owned());
                frame.destination = Some(target);
                frame.group_call = Some(group);
                frame.text = Some(lcn_text(lcn));
            }
            Self::Interconnect { target, group, lcn } => {
                frame.destination = Some(target);
                frame.group_call = Some(group);
                labelled_channel(&mut frame, "Interconnect", lcn);
            }
            Self::Login { group, unit } => {
                frame.opcode = Some("Login".to_owned());
                (frame.destination, frame.source) = (Some(group), Some(unit));
                frame.group_call = Some(true);
            }
            Self::Status { unit, status } => {
                frame.opcode = Some("Status".to_owned());
                frame.source = Some(unit);
                frame.text = Some(format!("status {status}"));
            }
            Self::Site { site, control_lcn } => {
                frame.opcode = Some("Site".to_owned());
                frame.site_id = Some(site);
                frame.text = Some(format!("control LCN {control_lcn}"));
            }
            Self::ExtendedSite { site, area } => {
                frame.opcode = Some("Site".to_owned());
                frame.site_id = Some(site);
                frame.text = Some(format!("area {area}"));
            }
            Self::System {
                system,
                control_lcn,
            } => {
                frame.opcode = Some("System".to_owned());
                frame.system_id = Some(system);
                frame.text = Some(format!("control LCN {control_lcn}"));
            }
            Self::AdjacentSite { site, lcn } => {
                frame.opcode = Some("Adjacent site".to_owned());
                frame.text = Some(format!("site {site} on LCN {lcn}"));
            }
            Self::Console { unit, lcn, drop } => {
                frame.source = Some(unit);
                let name = if drop {
                    "Console drop"
                } else {
                    "Console unkey"
                };
                labelled_channel(&mut frame, name, lcn);
            }
            Self::TestCall { lcn } => labelled_channel(&mut frame, "Test call", lcn),
            Self::Other(name) => frame.opcode = Some(name.to_owned()),
        }
        frame
    }
}

fn call_frame(frame: &mut DvFrame, call: Call, group: Option<bool>, name: &str) {
    frame.destination = (group.is_some()).then_some(call.target);
    frame.source = call.source.filter(|&source| source != 0);
    frame.group_call = group;
    frame.emergency = call.emergency.then_some(true);
    let name = match (call.digital, call.update) {
        (false, false) => name.to_owned(),
        (false, true) => format!("{name} update"),
        (true, false) => format!("Digital {}", name.to_lowercase()),
        (true, true) => format!("Digital {} update", name.to_lowercase()),
    };
    if call.digital {
        frame.opcode = Some(name);
        frame.text = Some(lcn_text(call.lcn));
    } else {
        labelled_channel(frame, &name, call.lcn);
    }
}

/// Names the message and the channel it assigns; a status where the channel goes is named
/// instead, so it is never followed.
fn labelled_channel(frame: &mut DvFrame, name: &str, lcn: u8) {
    match channel(lcn) {
        Ok(channel) => {
            frame.channel = Some(channel);
            frame.opcode = Some(name.to_owned());
        }
        Err(state) => frame.opcode = Some(format!("{name}, {state}")),
    }
}

fn lcn_text(lcn: u8) -> String {
    match channel(lcn) {
        Ok(channel) => format!("LCN {channel}"),
        Err(state) => state.to_owned(),
    }
}
