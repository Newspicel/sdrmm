use std::sync::Mutex;

use sdrmm_channels::ChannelError;
use sdrmm_device::DeviceError;
use sdrmm_wire::ChannelSettings;

use super::*;

const RATE: f64 = 48_000.0;
const RADIO: Band = Band {
    center_hz: 145_000_000.0,
    sample_rate: 240_000.0,
};

#[derive(Default)]
struct Aired {
    samples: Vec<Sample>,
    ends: usize,
    stopped: bool,
}

struct FakeRadio {
    aired: Arc<Mutex<Aired>>,
    takes: usize,
}

impl TxStream for FakeRadio {
    fn write_channels(
        &mut self,
        channels: &[&[Sample]],
        _timeout: Duration,
        end_burst: bool,
    ) -> Result<usize, DeviceError> {
        let samples = channels[0];
        let accepted = samples.len().min(self.takes);
        let mut aired = self.aired.lock().expect("aired");
        aired.samples.extend_from_slice(&samples[..accepted]);
        if end_burst && accepted == samples.len() {
            aired.ends += 1;
        }
        Ok(accepted)
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        self.aired.lock().expect("aired").stopped = true;
        Ok(())
    }
}

/// A carrier for as many samples as it was handed audio or frame octets.
#[derive(Default)]
struct Carrier {
    left: usize,
    endless: bool,
}

impl ChannelTx for Carrier {
    fn apply(&mut self, _settings: ChannelSettings) -> Result<(), ChannelError> {
        Ok(())
    }

    fn submit(&mut self, payload: TxPayload) -> Result<(), ChannelError> {
        self.left += match payload {
            TxPayload::Audio(pcm) => pcm.len(),
            TxPayload::Frame(frame) => frame.len(),
        };
        Ok(())
    }

    fn generate(&mut self, out: &mut [Sample]) -> usize {
        let take = if self.endless {
            out.len()
        } else {
            out.len().min(self.left)
        };
        out[..take].fill(Sample::new(1.0, 0.0));
        self.left = self.left.saturating_sub(take);
        take
    }
}

struct Rig {
    aired: Arc<Mutex<Aired>>,
    shared: Arc<TxShared>,
    commands: mpsc::Sender<TxCommand>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Rig {
    fn with(source: Source, settings: TransmitSettings) -> Self {
        let aired = Arc::new(Mutex::new(Aired::default()));
        let shared = Arc::new(TxShared::default());
        let radio = FakeRadio {
            aired: aired.clone(),
            takes: 1_000,
        };
        let worker = Worker::new(Box::new(radio), source, RADIO, settings, shared.clone());
        let (commands, received) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let thread = std::thread::spawn(move || worker.run(&received, &halt));
        Self {
            aired,
            shared,
            commands,
            stop,
            thread: Some(thread),
        }
    }

    fn modulator(carrier: Carrier, settings: TransmitSettings) -> Self {
        let source = Source::Modulator {
            tx: Box::new(carrier),
            rate: RATE,
        };
        Self::with(source, settings)
    }

    fn status(&self) -> TransmitStatus {
        self.shared.status("tx", &TransmitSettings::default())
    }

    fn until(&self, what: &str, done: impl Fn(&TransmitStatus) -> bool) -> TransmitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = self.status();
            if done(&status) {
                return status;
            }
            assert!(Instant::now() < deadline, "never {what}: {status:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("worker");
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.finish();
    }
}

fn frame(octets: usize) -> TxCommand {
    TxCommand::Payload(TxPayload::Frame(vec![0; octets]))
}

#[test]
fn a_frame_goes_out_as_one_burst_at_the_set_level_and_ends_it() {
    let mut rig = Rig::modulator(Carrier::default(), TransmitSettings::default());
    assert_eq!(rig.status().phase, TransmitPhase::Idle);
    rig.commands.send(frame(4_800)).expect("queued");
    let done = rig.until("ended the burst", |status| {
        status.bursts == 1 && status.phase == TransmitPhase::Idle && status.samples > 0
    });
    let ratio = RADIO.sample_rate / RATE;
    let carried = (4_800.0 * ratio) as u64;
    assert!(
        (carried..carried + 2_000).contains(&done.samples),
        "{} samples for a {carried} sample burst",
        done.samples
    );
    let occupied = done.occupied_hz.expect("a measured width");
    assert!(occupied < 500.0, "a bare carrier measured {occupied} Hz");
    assert_eq!(done.channel_hz, None, "no mode declared a width");
    let peak = done.peak_db.expect("a peak");
    assert!((peak - -3.0).abs() < 0.05, "peak {peak} dB");
    rig.finish();
    let aired = rig.aired.lock().expect("aired");
    assert_eq!(aired.ends, 1);
    assert!(aired.stopped);
    let settled = aired.samples[aired.samples.len() / 2].norm();
    assert!((0.70..0.72).contains(&settled), "aired level {settled}");
    assert!(aired.samples.last().expect("samples").norm() < 1e-3);
}

#[test]
fn a_modulator_is_paced_to_real_time_with_a_short_lead() {
    let rig = Rig::modulator(Carrier::default(), TransmitSettings::default());
    rig.commands.send(frame(48_000)).expect("queued");
    rig.until("keyed", |status| status.phase == TransmitPhase::OnAir);
    std::thread::sleep(Duration::from_millis(200));
    let sent = rig.status().samples as f64 / RADIO.sample_rate;
    assert!((0.15..0.45).contains(&sent), "{sent} s sent after 0.2 s");
}

#[test]
fn a_transmission_past_its_timeout_ends_and_disarms_with_the_reason() {
    let endless = Carrier {
        left: 0,
        endless: true,
    };
    let settings = TransmitSettings {
        timeout_s: 1,
        ..TransmitSettings::default()
    };
    let rig = Rig::modulator(endless, settings);
    let tripped = rig.until("tripped", |status| status.error.is_some());
    assert!(tripped.error.expect("reason").contains("1 s; disarmed"));
    rig.until("stopped", |_| rig.shared.stopped());
    assert_eq!(rig.status().phase, TransmitPhase::Idle);
    assert_eq!(rig.aired.lock().expect("aired").ends, 1);
}

#[test]
fn a_frequency_the_radio_cannot_reach_mutes_the_transmitter_until_it_moves_back() {
    let settings = TransmitSettings {
        frequency_hz: Some(RADIO.center_hz + 500_000.0),
        ..TransmitSettings::default()
    };
    let rig = Rig::modulator(Carrier::default(), settings);
    rig.commands.send(frame(2_400)).expect("queued");
    let muted = rig.until("refused the frequency", |status| status.error.is_some());
    assert_eq!(muted.frequency_hz, RADIO.center_hz + 500_000.0);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(rig.status().samples, 0);

    let reachable = TransmitSettings {
        frequency_hz: Some(RADIO.center_hz + 60_000.0),
        ..TransmitSettings::default()
    };
    rig.commands
        .send(TxCommand::Settings(reachable))
        .expect("retuned");
    rig.commands.send(frame(2_400)).expect("queued");
    let sent = rig.until("sent after the retune", |status| status.bursts == 1);
    assert_eq!(sent.error, None);
}

/// Short audio may key once the hang time has passed, so a status read only counts while the
/// test thread is provably still inside it.
#[test]
fn held_audio_waits_for_a_prefill_then_flows() {
    let rig = Rig::modulator(Carrier::default(), TransmitSettings::default());
    let short = (PREFILL_S * RATE) as usize / 3;
    let sent = Instant::now();
    rig.commands
        .send(TxCommand::Payload(TxPayload::Audio(vec![0.0; short])))
        .expect("audio");
    while sent.elapsed() < HANG / 2 {
        let bursts = rig.status().bursts;
        if sent.elapsed() < HANG {
            assert_eq!(bursts, 0, "keyed before the prefill");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..3 {
        rig.commands
            .send(TxCommand::Payload(TxPayload::Audio(vec![0.0; short])))
            .expect("audio");
    }
    rig.until("keyed", |status| status.bursts == 1);
}

#[test]
fn a_lane_is_sent_as_it_arrives_and_what_does_not_fit_is_counted() {
    let shared = Arc::new(TxShared::default());
    let band = Band {
        center_hz: RADIO.center_hz,
        sample_rate: RADIO.sample_rate,
    };
    let (mut tap, source) = feed(band, &shared);
    let aired = Arc::new(Mutex::new(Aired::default()));
    let radio = FakeRadio {
        aired: aired.clone(),
        takes: 100_000,
    };
    let worker = Worker::new(
        Box::new(radio),
        source,
        RADIO,
        TransmitSettings {
            level_db: 0.0,
            ..TransmitSettings::default()
        },
        shared.clone(),
    );
    let (commands, received) = mpsc::channel::<TxCommand>();
    let stop = Arc::new(AtomicBool::new(false));
    let halt = stop.clone();
    let thread = std::thread::spawn(move || worker.run(&received, &halt));

    let block = vec![Sample::new(0.25, 0.0); 24_000];
    assert!(tap.push(&block));
    let deadline = Instant::now() + Duration::from_secs(10);
    while aired.lock().expect("aired").ends == 0 {
        assert!(Instant::now() < deadline, "the lane never ended its burst");
        std::thread::sleep(Duration::from_millis(5));
    }
    {
        let aired = aired.lock().expect("aired");
        assert!(aired.samples.len() >= block.len());
        assert!((aired.samples[12_000].norm() - 0.25).abs() < 1e-3);
    }
    let flood = vec![Sample::new(0.0, 0.0); 1 << 20];
    assert!(tap.push(&flood), "an overflow drops samples, not the tap");
    assert_eq!(
        shared.status("tx", &TransmitSettings::default()).underruns,
        1
    );

    stop.store(true, Ordering::Release);
    thread.join().expect("worker");
    drop(commands);
    assert!(!tap.push(&block), "a tap with no transmitter lets go");
}

/// A gap past the hang time rightly drops the carrier, so only gaps the scheduler made that long
/// excuse a re-key.
#[test]
fn audio_that_arrives_late_is_bridged_and_the_carrier_stays_up() {
    let rig = Rig::modulator(Carrier::default(), TransmitSettings::default());
    let chunk = (RATE / 50.0) as usize;
    let mut stalls = 0;
    let mut last = Instant::now();
    for _ in 0..25 {
        let sending = Instant::now();
        rig.commands
            .send(TxCommand::Payload(TxPayload::Audio(vec![0.0; chunk])))
            .expect("audio");
        if last.elapsed() > HANG {
            stalls += 1;
        }
        last = sending;
        std::thread::sleep(Duration::from_millis(26));
    }
    let spoken = rig.status();
    if stalls == 0 && last.elapsed() < HANG {
        assert_eq!(spoken.phase, TransmitPhase::OnAir);
    }
    rig.until("unkeyed after the hang time", |status| {
        status.bursts > 0 && status.phase == TransmitPhase::Idle
    });
    let bursts = rig.status().bursts;
    assert!(
        bursts <= 1 + stalls,
        "slow audio keyed the carrier {bursts} times with {stalls} stalls"
    );
}
