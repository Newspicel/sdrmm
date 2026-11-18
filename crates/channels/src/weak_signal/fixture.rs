use super::*;
use crate::builtin::fixtures::{Heard, Positive, reads};
use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::settings;
use sdrmm_wire::ChannelParams;

pub(crate) fn ft8_decoder() -> Positive {
    reads(&[r#""text":"CQ W1AW FN42""#]).hearing(ft8_slot)
}

fn ft8_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::new("ft8", &WsjtParams::default()).expect("ft8 settings");
    let mut channel = Ft8Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::ft8_slot("W1AW", "FN42", 1_500.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

pub(crate) fn ft4_decoder() -> Positive {
    reads(&[r#""text":"CQ JA1ABC PM95""#]).hearing(ft4_slot)
}

fn ft4_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::new("ft4", &WsjtParams::default()).expect("ft4 settings");
    let mut channel = Ft4Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::ft4_slot("JA1ABC", "PM95", 1_000.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

pub(crate) fn wspr_decoder() -> Positive {
    reads(&[
        r#""callsign":"K1ABC""#,
        r#""grid":"FN42""#,
        r#""power_dbm":37"#,
    ])
    .hearing(wspr_slot)
}

fn wspr_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::new("wspr", &WsprParams::default()).expect("wspr settings");
    let mut channel = WsprChannel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::wspr_slot("K1ABC", "FN42", 37, 1_500.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}
