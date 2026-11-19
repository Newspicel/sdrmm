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

pub(crate) fn msk144_decoder() -> Positive {
    reads(&[r#""text":"CQ K1JT FN20""#]).hearing(msk144::tests::heard)
}

pub(crate) fn q65_decoder() -> Positive {
    reads(&[r#""text":"CQ W1AW FN42""#]).hearing(q65_slot)
}

fn q65_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::of(&Q65Params::default()).expect("q65 settings");
    let mut channel = Q65Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::q65_slot("W1AW", "FN42", 1_500.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

pub(crate) fn fst4_decoder() -> Positive {
    reads(&[r#""text":"CQ K1ABC FN42""#]).hearing(fst4_slot)
}

fn fst4_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::of(&Fst4Params::default()).expect("fst4 settings");
    let mut channel = Fst4Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::fst4_slot("K1ABC", "FN42", 1_200.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

pub(crate) fn fst4w_decoder() -> Positive {
    reads(&[r#""callsign":"K1ABC""#, r#""power_dbm":37"#]).hearing(fst4w_slot)
}

fn fst4w_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::of(&Fst4wParams::default()).expect("fst4w settings");
    let mut channel = Fst4wChannel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::fst4w_slot("K1ABC", "FN42", 37, 1_500.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

pub(crate) fn jt65_decoder() -> Positive {
    reads(&[r#""text":"CQ K1ABC FN42""#]).hearing(jt65_slot)
}

fn jt65_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::new("jt65", &WsjtParams::default()).expect("jt65 settings");
    let mut channel = Jt65Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::jt65_slot("K1ABC", "FN42", 1_200.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

pub(crate) fn jt9_decoder() -> Positive {
    reads(&[r#""text":"CQ K1ABC FN42""#]).hearing(jt9_slot)
}

fn jt9_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::new("jt9", &WsjtParams::default()).expect("jt9 settings");
    let mut channel = Jt9Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    channel.process(
        &synth::jt9_slot("K1ABC", "FN42", 1_200.0).expect("a synthetic signal"),
        &mut out,
    );
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}

/// The 12 kHz audio of a committed fixture, held as the in-phase part.
pub(crate) fn recording(stem: &str) -> Vec<f32> {
    let bytes = std::fs::read(format!(
        "{}/../../fixtures/{stem}.sigmf-data",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]))
        .collect()
}

pub(crate) fn js8_decoder() -> Positive {
    reads(&[r#""text":"KN4CRD: @HB HEARTBEAT EM73""#, r#""grid":"EM73""#]).hearing(js8_slot)
}

fn js8_slot() -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let params = ChannelParams::new("js8", &WsjtParams::default()).expect("js8 settings");
    let mut channel = Js8Channel::new(ctx, settings(params)).expect("builds");
    let mut out = ChannelOutputs::default();
    let payload = js8::pack::heartbeat("KN4CRD", "EM73", false, 3);
    channel.process(&js8::normal_slot(payload, 1_500.0), &mut out);
    channel.settle(&mut out);
    Heard::from_events(&out.events)
}
