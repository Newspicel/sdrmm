use super::{le_u16, le_u24, le_u32};

struct Command {
    cid: u8,
    uplink: bool,
    name: &'static str,
    len: usize,
    detail: fn(&[u8]) -> Option<String>,
}

const fn up(
    cid: u8,
    name: &'static str,
    len: usize,
    detail: fn(&[u8]) -> Option<String>,
) -> Command {
    Command {
        cid,
        uplink: true,
        name,
        len,
        detail,
    }
}

const fn down(
    cid: u8,
    name: &'static str,
    len: usize,
    detail: fn(&[u8]) -> Option<String>,
) -> Command {
    Command {
        cid,
        uplink: false,
        name,
        len,
        detail,
    }
}

const COMMANDS: &[Command] = &[
    up(0x01, "ResetInd", 1, version),
    down(0x01, "ResetConf", 1, version),
    up(0x02, "LinkCheckReq", 0, plain),
    down(0x02, "LinkCheckAns", 2, link_check_ans),
    down(0x03, "LinkADRReq", 4, link_adr_req),
    up(0x03, "LinkADRAns", 1, ack3),
    down(0x04, "DutyCycleReq", 1, duty_cycle_req),
    up(0x04, "DutyCycleAns", 0, plain),
    down(0x05, "RXParamSetupReq", 4, rx_param_setup_req),
    up(0x05, "RXParamSetupAns", 1, ack3),
    down(0x06, "DevStatusReq", 0, plain),
    up(0x06, "DevStatusAns", 2, dev_status_ans),
    down(0x07, "NewChannelReq", 5, new_channel_req),
    up(0x07, "NewChannelAns", 1, ack2),
    down(0x08, "RXTimingSetupReq", 1, rx_timing_setup_req),
    up(0x08, "RXTimingSetupAns", 0, plain),
    down(0x09, "TxParamSetupReq", 1, plain),
    up(0x09, "TxParamSetupAns", 0, plain),
    down(0x0a, "DlChannelReq", 4, dl_channel_req),
    up(0x0a, "DlChannelAns", 1, ack2),
    up(0x0b, "RekeyInd", 1, version),
    down(0x0b, "RekeyConf", 1, version),
    down(0x0c, "ADRParamSetupReq", 1, plain),
    up(0x0c, "ADRParamSetupAns", 0, plain),
    up(0x0d, "DeviceTimeReq", 0, plain),
    down(0x0d, "DeviceTimeAns", 5, device_time_ans),
    down(0x0e, "ForceRejoinReq", 2, plain),
    down(0x0f, "RejoinParamSetupReq", 1, plain),
    up(0x0f, "RejoinParamSetupAns", 1, ack1),
    up(0x10, "PingSlotInfoReq", 1, plain),
    down(0x10, "PingSlotInfoAns", 0, plain),
    down(0x11, "PingSlotChannelReq", 4, frequency_detail),
    up(0x11, "PingSlotChannelAns", 1, ack2),
    up(0x12, "BeaconTimingReq", 0, plain),
    down(0x12, "BeaconTimingAns", 3, plain),
    down(0x13, "BeaconFreqReq", 3, frequency_detail),
    up(0x13, "BeaconFreqAns", 1, ack1),
    up(0x20, "DeviceModeInd", 1, device_class),
    down(0x20, "DeviceModeConf", 1, device_class),
];

pub(super) fn parse(bytes: &[u8], uplink: bool) -> Vec<String> {
    let mut commands = Vec::new();
    let mut rest = bytes;
    while let Some((&cid, body)) = rest.split_first() {
        let Some(command) = COMMANDS
            .iter()
            .find(|command| command.cid == cid && command.uplink == uplink)
        else {
            commands.push(format!("unknown 0x{cid:02x}"));
            break;
        };
        let (Some(args), Some(after)) = (body.get(..command.len), body.get(command.len..)) else {
            commands.push(format!("{} truncated", command.name));
            break;
        };
        commands.push(match (command.detail)(args) {
            Some(detail) => format!("{} {detail}", command.name),
            None => command.name.to_owned(),
        });
        rest = after;
    }
    commands
}

fn plain(_: &[u8]) -> Option<String> {
    None
}

fn megahertz(bytes: Option<&[u8]>) -> Option<String> {
    let hz = f64::from(le_u24(bytes)?) * 100.0;
    Some(format!("{} MHz", hz / 1e6))
}

fn version(args: &[u8]) -> Option<String> {
    Some(format!("1.{}", args.first()? & 0x0f))
}

fn link_check_ans(args: &[u8]) -> Option<String> {
    Some(format!("margin {} dB gw {}", args.first()?, args.get(1)?))
}

fn link_adr_req(args: &[u8]) -> Option<String> {
    let rate_power = args.first()?;
    let mask = le_u16(args.get(1..3))?;
    Some(format!(
        "DR{} TXPower {} ChMask {mask:04x}",
        rate_power >> 4,
        rate_power & 0x0f
    ))
}

fn acks(args: &[u8], mask: u8) -> Option<String> {
    let accepted = args.first()? & mask == mask;
    Some(if accepted { "ok" } else { "rejected" }.to_owned())
}

fn ack1(args: &[u8]) -> Option<String> {
    acks(args, 0b001)
}

fn ack2(args: &[u8]) -> Option<String> {
    acks(args, 0b011)
}

fn ack3(args: &[u8]) -> Option<String> {
    acks(args, 0b111)
}

fn duty_cycle_req(args: &[u8]) -> Option<String> {
    Some(format!("1/{}", 1u32 << (args.first()? & 0x0f)))
}

fn rx_param_setup_req(args: &[u8]) -> Option<String> {
    let settings = args.first()?;
    Some(format!(
        "RX1DROffset {} RX2 DR{} {}",
        (settings >> 4) & 0x07,
        settings & 0x0f,
        megahertz(args.get(1..4))?
    ))
}

fn dev_status_ans(args: &[u8]) -> Option<String> {
    let battery = args.first()?;
    let margin = ((args.get(1)? << 2).cast_signed()) >> 2;
    Some(format!("battery {battery} margin {margin}"))
}

fn new_channel_req(args: &[u8]) -> Option<String> {
    let range = args.get(4)?;
    Some(format!(
        "ch {} {} DR{}-{}",
        args.first()?,
        megahertz(args.get(1..4))?,
        range & 0x0f,
        range >> 4
    ))
}

fn rx_timing_setup_req(args: &[u8]) -> Option<String> {
    Some(format!("delay {} s", (args.first()? & 0x0f).max(1)))
}

fn dl_channel_req(args: &[u8]) -> Option<String> {
    Some(format!(
        "ch {} {}",
        args.first()?,
        megahertz(args.get(1..4))?
    ))
}

fn device_time_ans(args: &[u8]) -> Option<String> {
    Some(format!("GPS {} s", le_u32(args.get(..4))?))
}

fn frequency_detail(args: &[u8]) -> Option<String> {
    megahertz(args.get(..3))
}

fn device_class(args: &[u8]) -> Option<String> {
    Some(
        match args.first()? {
            0 => "class A",
            2 => "class C",
            _ => "class RFU",
        }
        .to_owned(),
    )
}
