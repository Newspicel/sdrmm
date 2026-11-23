use std::sync::Mutex;

use sdrmm_decoder::encoder::EncoderError;

use super::*;
use crate::transmit::{TxShared, encoded_for_tests};

const RATE: f64 = 48_000.0;

/// A carrier for as many samples as it was handed audio or frame octets.
#[derive(Default)]
struct Carrier {
    left: Arc<Mutex<usize>>,
}

impl Modulator for Carrier {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        *self.left.lock().expect("left") += match payload {
            TxPayload::Audio(pcm) => pcm.len(),
            TxPayload::Frame(frame) => frame.len(),
        };
        Ok(())
    }

    fn generate(&mut self, out: &mut [Sample]) -> usize {
        let mut left = self.left.lock().expect("left");
        let take = out.len().min(*left);
        out[..take].fill(Sample::new(1.0, 0.0));
        *left -= take;
        take
    }
}

fn run() -> Run {
    Run::new(
        Box::new(Carrier::default()),
        RATE,
        Arc::new(EncoderShared::default()),
    )
}

fn produced(run: &Run) -> f64 {
    run.produced as f64 / RATE
}

fn ahead(run: &Run) -> f64 {
    produced(run) - run.started.elapsed().as_secs_f64()
}

#[test]
fn an_idle_encoder_keeps_real_time_and_a_message_starts_a_lead_ahead() {
    let mut run = run();
    std::thread::sleep(Duration::from_millis(30));
    while run.step() {}
    assert!(
        (-0.01..=0.01).contains(&ahead(&run)),
        "{} s ahead",
        ahead(&run)
    );
    run.obey(Command::Payload(TxPayload::Frame(vec![0; 48_000])));
    while run.step() {}
    let lead = ahead(&run);
    assert!(
        (MESSAGE_LEAD_S - 0.03..=MESSAGE_LEAD_S + 0.01).contains(&lead),
        "{lead} s ahead"
    );
}

#[test]
fn a_recorder_takes_the_stream_whole_and_a_transmitter_only_the_burst() {
    let mut run = run();
    let shared = Arc::new(TxShared::default());
    let (air, mut aired) = encoded_for_tests(RATE, &shared);
    run.obey(Command::Attach(Sink::Air(air)));
    run.obey(Command::Payload(TxPayload::Frame(vec![0; 480])));
    while run.step() {}
    assert!(run.produced > 480, "the stream runs past the burst");
    let (keyed, still_keyed) = aired();
    assert_eq!(keyed, 480);
    assert!(!still_keyed, "the burst ended with the frame");
}

#[test]
fn held_audio_waits_for_a_prefill_then_flows() {
    let mut run = run();
    let short = (PREFILL_S * RATE) as usize / 3;
    run.obey(Command::Payload(TxPayload::Audio(vec![0.0; short])));
    run.release_audio();
    assert!(!run.flowing, "keyed before the prefill");
    for _ in 0..3 {
        run.obey(Command::Payload(TxPayload::Audio(vec![0.0; short])));
    }
    run.release_audio();
    assert!(run.flowing);
}

#[test]
fn audio_that_arrives_late_is_bridged_with_silence() {
    let mut run = run();
    run.obey(Command::Payload(TxPayload::Audio(vec![0.0; 4_800])));
    run.release_audio();
    run.voiced = 0;
    run.bridge(960);
    assert_eq!(run.voiced, 960, "a short gap is covered");
}
