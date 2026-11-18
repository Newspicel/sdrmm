use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_decoder::encoder::{Modulator, NativeModulator, TxPayload};
use sdrmm_wire::ChannelParams;

const APRS_POSITION_REPORT: &str = "!5230.00N/01324.00E>SDR FIXTURE";

pub(crate) fn aprs_decoder() -> Positive {
    reads(&[
        r#""source":"DL0SDR""#,
        r#""destination":"APRS""#,
        r#""info":"!5230.00N/01324.00E>SDR FIXTURE""#,
    ])
    .at(52.5, 13.4)
    .hearing(|| signal().expect("aprs fixture").heard())
}

fn signal() -> Option<Fixture> {
    let params = ChannelParams::new(
        "aprs",
        &AprsParams {
            mode: AprsMode::Afsk1200,
            ..AprsParams::default()
        },
    )
    .expect("aprs settings");
    let rate = Fixture::rate(&params);
    let mut tx = AprsModulator::new(&AprsEncoderSettings {
        mode: AprsMode::Afsk1200,
    })
    .ok()?;
    tx.submit(TxPayload::Frame(aprs_frame(APRS_POSITION_REPORT)))
        .ok()?;
    let iq = sdrmm_decoder::encoder::burst(&mut tx);
    Some(Fixture::new(params, with_tail(iq, rate, 0.5)))
}
fn aprs_frame(text: &str) -> Vec<u8> {
    let mut frame = Vec::new();
    for call in ["APRS  ", "DL0SDR"] {
        frame.extend(call.bytes().map(|byte| byte << 1));
        frame.push(0x60);
    }
    let last = frame.len() - 1;
    frame[last] |= 1;
    frame.extend([0x03, 0xF0]);
    frame.extend(text.bytes());
    frame
}
