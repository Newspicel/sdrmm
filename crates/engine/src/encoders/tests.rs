use std::{collections::HashSet, time::Duration};

use super::*;

fn spec(type_id: &str) -> EncoderSpec {
    EncoderSpec {
        params: EncoderParams::bare(type_id),
        audio: AudioProcessing::default(),
    }
}

fn running(dir: &tempfile::TempDir) -> Arc<Engine> {
    let engine = Engine::new(Some(dir.path().to_path_buf()));
    engine.run_encoder("enc", &spec("nfm")).expect("encoder");
    engine
}

#[test]
fn an_encoder_records_its_baseband_with_no_transmitter() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = running(&dir);
    engine
        .start_encoder_baseband_recording("enc")
        .expect("recording");
    assert!(engine.start_encoder_baseband_recording("enc").is_err());
    engine
        .encoder_audio("enc", 1, vec![0.25; 4_800])
        .expect("audio taken");
    std::thread::sleep(Duration::from_millis(150));
    let live = engine.snapshot();
    assert_eq!(live.node_streams[0].node, "enc");
    assert!(live.node_streams[0].baseband_recording.is_some());
    let done = engine
        .stop_encoder_baseband_recording("enc")
        .expect("stopped");
    assert!(done.samples > 0, "{done:?}");
    assert_eq!(done.error, None);
    assert!(engine.snapshot().node_streams.is_empty());
}

#[test]
fn an_encoder_streams_its_baseband_to_the_network() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = running(&dir);
    let receiver = std::net::UdpSocket::bind("127.0.0.1:0").expect("receiver");
    receiver
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let settings = NetworkExportSettings {
        address: receiver.local_addr().expect("address").to_string(),
        transport: sdrmm_wire::NetworkTransport::Udp,
        format: sdrmm_wire::NetworkSampleFormat::Cf32Le,
    };
    engine
        .start_encoder_network_export("enc", "net".to_owned(), settings)
        .expect("exporting");
    let mut packet = [0u8; 2_048];
    assert!(receiver.recv(&mut packet).expect("a packet") > 0);
    let stopped = engine
        .stop_encoder_network_export("enc", "net")
        .expect("stopped");
    assert!(stopped.samples > 0);
}

#[test]
fn a_stopped_encoder_takes_nothing_more() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = running(&dir);
    assert!(engine.runs_encoder("enc"));
    engine.retain_encoders(&HashSet::new());
    assert!(!engine.runs_encoder("enc"));
    assert!(matches!(
        engine.encoder_audio("enc", 1, vec![0.0; 960]),
        Err(EngineError::EncoderNotRunning(_))
    ));
}

#[test]
fn new_settings_retune_in_place_and_a_new_mode_starts_over() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = running(&dir);
    engine
        .start_encoder_baseband_recording("enc")
        .expect("recording");
    let louder = EncoderSpec {
        audio: AudioProcessing {
            auto_notch: true,
            ..AudioProcessing::default()
        },
        ..spec("nfm")
    };
    engine.run_encoder("enc", &louder).expect("retuned");
    assert!(
        engine.snapshot().node_streams[0]
            .baseband_recording
            .is_some()
    );
    engine.run_encoder("enc", &spec("aprs")).expect("restarted");
    assert!(engine.snapshot().node_streams.is_empty());
}
