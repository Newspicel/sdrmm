#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use sdrmm_channels::{AprsPacket, AprsParams, NfmParams};
use sdrmm_device::DeviceRegistry;
use sdrmm_device_virtual::{NFM_CARRIER_OFFSET_HZ, VirtualDriver};
use sdrmm_engine::{Engine, EngineError, TransmitRequest, TransmitSource, audio::PcmPayload};
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DeviceSettings, Squelch, TransmitPhase, TransmitSettings,
    TransmitStatus, Tuning,
};

const WAIT: Duration = Duration::from_secs(30);
const AUDIO_RATE: f64 = 48_000.0;
const TONE_HZ: f64 = 1_000.0;

fn engine() -> Arc<Engine> {
    let mut registry = DeviceRegistry::new();
    registry.register(10, Box::new(VirtualDriver::new()));
    Engine::with_registry(registry, None)
}

fn radio(engine: &Engine, device: &str, center_hz: f64) -> u32 {
    let ds = engine.create_device_set(device).expect("radio");
    engine
        .patch_device(
            ds,
            DeviceSettings {
                center_hz: Some(center_hz),
                tuning: Some(Tuning::Manual),
                ..DeviceSettings::default()
            },
        )
        .expect("tuned");
    ds
}

fn listening(frequency_hz: f64, params: ChannelParams) -> ChannelSettings {
    ChannelSettings {
        frequency_hz,
        squelch: Squelch::Off,
        params,
        blanker: Default::default(),
        if_filter: Default::default(),
    }
}

fn request(node: &str, source: TransmitSource, frequency_hz: Option<f64>) -> TransmitRequest {
    TransmitRequest {
        node: node.to_owned(),
        source,
        settings: TransmitSettings {
            frequency_hz,
            ..TransmitSettings::default()
        },
    }
}

fn status(engine: &Engine, ds: u32) -> Option<TransmitStatus> {
    engine
        .snapshot()
        .device_sets
        .into_iter()
        .find(|set| set.id == ds)
        .and_then(|set| set.transmit)
}

fn settled(engine: &Engine, ds: u32, done: impl Fn(&TransmitStatus) -> bool) -> TransmitStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        let now = status(engine, ds).expect("an armed transmitter");
        if done(&now) {
            return now;
        }
        assert!(Instant::now() < deadline, "transmitter stuck at {now:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How much of a window is the test tone: near one for the tone alone, near zero for noise.
fn tone_share(samples: &[f32]) -> f32 {
    let (mut re, mut im, mut power) = (0.0f64, 0.0f64, 0.0f64);
    for (n, sample) in samples.iter().enumerate() {
        let phase = std::f64::consts::TAU * TONE_HZ * n as f64 / AUDIO_RATE;
        re += f64::from(*sample) * phase.cos();
        im += f64::from(*sample) * phase.sin();
        power += f64::from(*sample).powi(2);
    }
    let tone = 2.0 * re.hypot(im) / samples.len() as f64;
    let rms = (power / samples.len() as f64).sqrt();
    if rms < 1e-4 {
        return 0.0;
    }
    (tone / (std::f64::consts::SQRT_2 * rms)) as f32
}

fn heard_tone(audio: &mut tokio::sync::broadcast::Receiver<sdrmm_engine::PcmBlock>) -> f32 {
    let mut heard = Vec::new();
    loop {
        match audio.try_recv() {
            Ok(block) => match block.payload {
                PcmPayload::Samples(samples) => heard.extend_from_slice(&samples),
                PcmPayload::Silence(frames) => heard.resize(heard.len() + frames, 0.0),
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
            Err(_) => break,
        }
    }
    let window = (AUDIO_RATE / 10.0) as usize;
    heard
        .chunks_exact(window)
        .map(tone_share)
        .fold(0.0, f32::max)
}

#[tokio::test]
async fn a_message_sent_from_a_half_duplex_radio_is_decoded_by_another() {
    const CENTER_HZ: f64 = 144_600_000.0;
    const CHANNEL_HZ: f64 = CENTER_HZ + 270_000.0;
    let engine = engine();
    let talker = radio(&engine, "virtual:halfduplex", CENTER_HZ);
    let listener = radio(&engine, "virtual:transceiver", CENTER_HZ);
    let aprs = ChannelParams::new("aprs", &AprsParams::default()).expect("aprs settings");
    let mut decoded = engine.subscribe_decoded();
    let channel = engine
        .add_channel(listener, 0, listening(CHANNEL_HZ, aprs.clone()))
        .expect("decoder");

    let armed = engine
        .arm_transmitter(
            talker,
            &request("tx", TransmitSource::Modulator(aprs), Some(CHANNEL_HZ)),
        )
        .expect("armed");
    assert_eq!(armed.phase, TransmitPhase::Idle);
    assert_eq!(armed.frequency_hz, CHANNEL_HZ);
    tokio::time::sleep(Duration::from_millis(300)).await;
    engine
        .transmit_message(
            talker,
            "tx",
            &serde_json::json!({
                "source": "DL1ABC-9",
                "path": "WIDE1-1",
                "text": ">on the air from sdr--",
            }),
        )
        .expect("queued");

    let packet = tokio::time::timeout(WAIT, async {
        loop {
            let record = decoded.recv().await.expect("decoded stream");
            if record.device_set == listener
                && record.channel == channel
                && let Some(packet) = record.event.read::<AprsPacket>()
            {
                return packet;
            }
        }
    })
    .await
    .expect("the packet within the timeout");
    assert_eq!(packet.source, "DL1ABC-9");
    assert_eq!(packet.destination, "APRS");
    assert_eq!(packet.info, ">on the air from sdr--");

    let done = settled(&engine, talker, |now| {
        now.bursts == 1 && now.phase == TransmitPhase::Idle
    });
    assert!(done.samples > 0);
    assert_eq!(done.error, None);
    assert!(
        done.peak_db
            .is_some_and(|peak| (-3.5..-2.5).contains(&peak))
    );

    engine.disarm_transmitter(talker, "tx").expect("disarmed");
    assert_eq!(status(&engine, talker), None);
    assert!(matches!(
        engine.transmit_message(talker, "tx", &serde_json::json!({})),
        Err(EngineError::Transmit(_))
    ));
}

#[tokio::test]
async fn voice_fed_to_an_nfm_transmitter_comes_back_out_of_the_receiver() {
    const CENTER_HZ: f64 = 433_500_000.0;
    const CHANNEL_HZ: f64 = CENTER_HZ - 310_000.0;
    let engine = engine();
    let ds = radio(&engine, "virtual:transceiver", CENTER_HZ);
    let nfm = ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings");
    let channel = engine
        .add_channel(ds, 0, listening(CHANNEL_HZ, nfm.clone()))
        .expect("receiver");
    let mut audio = engine.subscribe_pcm(ds, channel).expect("PCM");
    engine
        .arm_transmitter(
            ds,
            &request("tx", TransmitSource::Modulator(nfm), Some(CHANNEL_HZ)),
        )
        .expect("armed");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(heard_tone(&mut audio) < 0.4, "a tone before anyone spoke");

    let chunk = (AUDIO_RATE / 50.0) as usize;
    let mut spoken = 0usize;
    let mut pace = tokio::time::interval(Duration::from_millis(20));
    for _ in 0..75 {
        let pcm = (spoken..spoken + chunk)
            .map(|n| (0.5 * (std::f64::consts::TAU * TONE_HZ * n as f64 / AUDIO_RATE).sin()) as f32)
            .collect();
        spoken += chunk;
        engine.transmit_audio(ds, "tx", pcm).expect("audio taken");
        pace.tick().await;
    }
    let keyed = settled(&engine, ds, |now| now.bursts >= 1);
    assert_eq!(keyed.error, None);
    let measured = settled(&engine, ds, |now| now.occupied_hz.is_some());
    let (occupied, channel) = (
        measured.occupied_hz.expect("a measured width"),
        measured.channel_hz.expect("the mode's width"),
    );
    assert!(
        (2_000.0..channel).contains(&occupied),
        "a voice tone measured {occupied} Hz in a {channel} Hz channel"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let level = heard_tone(&mut audio);
    assert!(level > 0.65, "the spoken tone came back at {level}");
    settled(&engine, ds, |now| now.phase == TransmitPhase::Idle);
    engine.remove_device_set(ds).expect("closed while armed");
}

#[tokio::test]
async fn a_lane_wired_to_a_transmitter_is_sent_as_it_is_received() {
    const CENTER_HZ: f64 = 100_000_000.0;
    let engine = engine();
    let source = radio(&engine, "virtual:band", CENTER_HZ);
    let ds = radio(&engine, "virtual:transceiver", CENTER_HZ);
    let nfm = ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings");
    let channel = engine
        .add_channel(ds, 0, listening(CENTER_HZ + NFM_CARRIER_OFFSET_HZ, nfm))
        .expect("receiver");
    let mut audio = engine.subscribe_pcm(ds, channel).expect("PCM");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        heard_tone(&mut audio) < 0.4,
        "the band's carrier without a wire"
    );

    let lane = TransmitSource::Lane {
        device_set: source,
        stream: 0,
    };
    let armed = engine
        .arm_transmitter(ds, &request("replay", lane, None))
        .expect("armed");
    assert_eq!(armed.frequency_hz, CENTER_HZ);
    settled(&engine, ds, |now| now.phase == TransmitPhase::OnAir);
    tokio::time::sleep(Duration::from_millis(800)).await;
    let level = heard_tone(&mut audio);
    assert!(level > 0.8, "the band's modulation came back at {level}");

    engine.remove_device_set(source).expect("source closed");
    let silent = settled(&engine, ds, |now| now.phase == TransmitPhase::Idle);
    assert_eq!(silent.bursts, 1);
}

#[tokio::test]
async fn a_channels_baseband_is_sent_on_another_frequency() {
    const HEARD_HZ: f64 = 100_000_000.0;
    const SENT_HZ: f64 = 446_000_000.0;
    let engine = engine();
    let source = radio(&engine, "virtual:band", HEARD_HZ);
    let ds = radio(&engine, "virtual:transceiver", SENT_HZ - 200_000.0);
    let nfm = || ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings");
    let heard = engine
        .add_channel(
            source,
            0,
            listening(HEARD_HZ + NFM_CARRIER_OFFSET_HZ, nfm()),
        )
        .expect("the channel that is repeated");
    let monitor = engine
        .add_channel(ds, 0, listening(SENT_HZ, nfm()))
        .expect("receiver");
    let mut audio = engine.subscribe_pcm(ds, monitor).expect("PCM");

    let translated = TransmitSource::Channel {
        device_set: source,
        channel: heard,
    };
    let armed = engine
        .arm_transmitter(ds, &request("translator", translated, Some(SENT_HZ)))
        .expect("armed");
    assert_eq!(armed.frequency_hz, SENT_HZ);
    assert_eq!(armed.channel_hz, None, "a channel declares no mask");
    settled(&engine, ds, |now| now.phase == TransmitPhase::OnAir);
    tokio::time::sleep(Duration::from_millis(900)).await;
    let level = heard_tone(&mut audio);
    assert!(level > 0.65, "the repeated channel came back at {level}");

    engine
        .remove_channel(source, heard)
        .expect("channel removed");
    settled(&engine, ds, |now| now.phase == TransmitPhase::Idle);
    assert!(matches!(
        engine.arm_transmitter(
            ds,
            &request(
                "translator",
                TransmitSource::Channel {
                    device_set: source,
                    channel: heard,
                },
                Some(SENT_HZ),
            ),
        ),
        Err(EngineError::ChannelNotFound(..))
    ));
}

#[test]
fn what_cannot_be_sent_is_refused_with_its_reason() {
    let engine = engine();
    let nfm = || TransmitSource::Modulator(ChannelParams::bare("nfm"));
    let quad = radio(&engine, "virtual:quad", 100_000_000.0);
    let refused = |result: Result<TransmitStatus, EngineError>, needle: &str| match result {
        Err(EngineError::Transmit(reason)) => assert!(reason.contains(needle), "{reason}"),
        other => panic!("must be refused for {needle}, got {other:?}"),
    };
    refused(
        engine.arm_transmitter(quad, &request("tx", nfm(), None)),
        "does not transmit",
    );
    let ds = radio(&engine, "virtual:transceiver", 100_000_000.0);
    refused(
        engine.arm_transmitter(
            ds,
            &request(
                "tx",
                TransmitSource::Modulator(ChannelParams::bare("adsb")),
                None,
            ),
        ),
        "no modulator",
    );
    let mut loud = request("tx", nfm(), None);
    loud.settings.level_db = 6.0;
    refused(engine.arm_transmitter(ds, &loud), "level");

    engine
        .arm_transmitter(ds, &request("tx", nfm(), None))
        .expect("armed");
    refused(
        engine.arm_transmitter(ds, &request("other", nfm(), None)),
        "already carries",
    );
    let far = engine
        .patch_transmitter(
            ds,
            "tx",
            TransmitSettings {
                frequency_hz: Some(105_000_000.0),
                ..TransmitSettings::default()
            },
        )
        .expect("the setting is taken");
    assert_eq!(far.settings.frequency_hz, Some(105_000_000.0));
    let muted = settled(&engine, ds, |now| now.error.is_some());
    assert!(
        muted
            .error
            .expect("reason")
            .contains("from the radio's centre")
    );
    engine
        .arm_transmitter(ds, &request("tx", nfm(), None))
        .expect("re-arming the same node replaces it");
    assert_eq!(status(&engine, ds).expect("armed").error, None);
}
