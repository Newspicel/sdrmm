use std::sync::Mutex;

use sdrmm_device::DeviceError;

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
    pause: Duration,
}

impl TxStream for FakeRadio {
    fn write_channels(
        &mut self,
        channels: &[&[Sample]],
        _timeout: Duration,
        end_burst: bool,
    ) -> Result<usize, DeviceError> {
        let samples = channels[0];
        if !end_burst {
            std::thread::sleep(self.pause);
        }
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

struct Rig {
    aired: Arc<Mutex<Aired>>,
    shared: Arc<TxShared>,
    commands: mpsc::Sender<TxCommand>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    air: Option<AirTap>,
}

impl Rig {
    fn encoded(settings: TransmitSettings) -> Self {
        let shared = Arc::new(TxShared::default());
        let (air, source) = encoded(RATE, &shared);
        let aired = Arc::new(Mutex::new(Aired::default()));
        let radio = FakeRadio {
            aired: aired.clone(),
            takes: 1_000,
            pause: Duration::ZERO,
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
            air: Some(air),
        }
    }

    /// A keyed burst of `len` full-scale samples, then the end of it.
    fn burst(&mut self, len: usize) {
        let air = self.air.as_mut().expect("an encoder feeds this rig");
        let ones = vec![Sample::new(1.0, 0.0); len];
        assert!(air.push(&ones, len));
        assert!(air.push(&[Sample::new(0.0, 0.0)], 0));
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

#[test]
fn a_burst_goes_out_at_the_set_level_and_ends_when_the_encoder_unkeys() {
    let mut rig = Rig::encoded(TransmitSettings::default());
    assert_eq!(rig.status().phase, TransmitPhase::Idle);
    rig.burst(4_800);
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
fn a_transmission_past_its_timeout_ends_and_disarms_with_the_reason() {
    let settings = TransmitSettings {
        timeout_s: 1,
        ..TransmitSettings::default()
    };
    let mut rig = Rig::encoded(settings);
    let mut air = rig.air.take().expect("an encoder feeds this rig");
    let block = vec![Sample::new(1.0, 0.0); 960];
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.status().error.is_none() {
        assert!(Instant::now() < deadline, "never tripped");
        let _ = air.push(&block, block.len());
        std::thread::sleep(Duration::from_millis(15));
    }
    let tripped = rig.status();
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
    let mut rig = Rig::encoded(settings);
    rig.burst(2_400);
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
    std::thread::sleep(Duration::from_millis(50));
    rig.burst(2_400);
    let sent = rig.until("sent after the retune", |status| status.bursts == 1);
    assert_eq!(sent.error, None);
}

#[test]
fn a_lane_is_sent_as_it_arrives_and_what_does_not_fit_is_counted() {
    let shared = Arc::new(TxShared::default());
    let band = RADIO;
    let (mut tap, source) = feed(band, &shared);
    let aired = Arc::new(Mutex::new(Aired::default()));
    let radio = FakeRadio {
        aired: aired.clone(),
        takes: 100_000,
        pause: Duration::ZERO,
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

#[test]
fn a_lane_with_samples_waiting_stays_one_burst_while_the_radio_is_slow_to_take_them() {
    let shared = Arc::new(TxShared::default());
    let (mut tap, source) = feed(RADIO, &shared);
    let aired = Arc::new(Mutex::new(Aired::default()));
    let radio = FakeRadio {
        aired: aired.clone(),
        takes: 100_000,
        pause: HANG + Duration::from_millis(80),
    };
    let worker = Worker::new(
        Box::new(radio),
        source,
        RADIO,
        TransmitSettings::default(),
        shared.clone(),
    );
    let (_commands, received) = mpsc::channel::<TxCommand>();
    let stop = Arc::new(AtomicBool::new(false));
    let halt = stop.clone();
    let thread = std::thread::spawn(move || worker.run(&received, &halt));

    let lane = vec![Sample::new(0.25, 0.0); 72_000];
    assert!(tap.push(&lane));
    let deadline = Instant::now() + Duration::from_secs(10);
    while aired.lock().expect("aired").ends == 0 {
        assert!(Instant::now() < deadline, "the lane never ended its burst");
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Release);
    thread.join().expect("worker");
    let status = shared.status("tx", &TransmitSettings::default());
    assert_eq!(status.bursts, 1);
    let aired = aired.lock().expect("aired");
    assert_eq!(aired.ends, 1);
    assert!(aired.samples.len() >= lane.len());
}
