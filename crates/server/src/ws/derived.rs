use num_complex::Complex;
use sdrmm_dsp::{
    DbWindowSmoother, PowerAverage, SpanFloor, SpectrumAnalyzer, adaptive_db_window,
    decimate_signal, quantize_db,
};
use sdrmm_engine::{IqBlock, PcmBlock, PcmPayload};
use sdrmm_wire::{SPECTRUM_SIGNAL_MARGIN_DB, ServerEvent, SpectrumFrame, StreamKind};
use tokio::sync::broadcast;

use super::{FrameThrottle, MAX_BINS, MAX_FPS, MIN_BINS, Outbox, text_event};

pub(super) const IQ_FFT: usize = 2048;
pub(super) const AUDIO_FFT: usize = 1024;

const AUDIO_RATE: f32 = sdrmm_channels::AUDIO_RATE as f32;

struct Ring {
    buf: Vec<Complex<f32>>,
    write: usize,
    filled: usize,
}

impl Ring {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![Complex::new(0.0, 0.0); size.max(1)],
            write: 0,
            filled: 0,
        }
    }

    fn push(&mut self, sample: Complex<f32>) {
        self.buf[self.write] = sample;
        self.write = (self.write + 1) % self.buf.len();
        self.filled = (self.filled + 1).min(self.buf.len());
    }

    fn ready(&self) -> bool {
        self.filled == self.buf.len()
    }

    fn oldest_first(&self, out: &mut [Complex<f32>]) {
        let size = self.buf.len();
        let (tail, head) = self.buf.split_at(self.write);
        out[..head.len()].copy_from_slice(head);
        out[size - self.write..].copy_from_slice(tail);
    }
}

struct Rows {
    analyzer: SpectrumAnalyzer,
    input: Vec<Complex<f32>>,
    db: Vec<f32>,
    average: PowerAverage,
    held: Vec<f32>,
    dec: Vec<f32>,
    quant: Vec<u8>,
    window: Vec<f32>,
    smoother: DbWindowSmoother,
    span_floor: SpanFloor,
}

impl Rows {
    fn new(size: usize, bins: usize, positive_only: bool) -> Self {
        let lanes = if positive_only { size / 2 } else { size };
        Self {
            analyzer: SpectrumAnalyzer::new(size),
            input: vec![Complex::new(0.0, 0.0); size],
            db: vec![f32::NEG_INFINITY; size],
            average: PowerAverage::new(lanes),
            held: vec![f32::NEG_INFINITY; lanes],
            dec: vec![0.0; bins],
            quant: vec![0; bins],
            window: Vec::with_capacity(bins),
            smoother: DbWindowSmoother::default(),
            span_floor: SpanFloor::default(),
        }
    }

    fn accumulate(&mut self, ring: &Ring) {
        ring.oldest_first(&mut self.input);
        self.analyzer.power_db(&self.input, &mut self.db);
        let lanes = self.held.len();
        self.average.add(&self.db[self.db.len() - lanes..]);
    }

    fn ready(&self) -> bool {
        self.average.count() > 0
    }

    fn take(&mut self) -> (f32, f32, f32, &[u8]) {
        self.average.take_db(&mut self.held);
        let floor_db = self
            .span_floor
            .read(&self.held)
            .unwrap_or(f32::NEG_INFINITY);
        decimate_signal(
            &self.held,
            floor_db + SPECTRUM_SIGNAL_MARGIN_DB,
            &mut self.dec,
        );
        let (db_min, db_max) = self
            .smoother
            .follow(adaptive_db_window(&self.dec, &mut self.window));
        quantize_db(&self.dec, db_min, db_max, &mut self.quant);
        (db_min, db_max, floor_db, &self.quant)
    }
}

fn limits(fps: u16, bins: u16) -> (u16, usize) {
    (
        fps.clamp(1, MAX_FPS),
        (bins as usize).clamp(MIN_BINS, MAX_BINS),
    )
}

async fn stopped(stream_id: u16, out_tx: &Outbox) {
    let event = ServerEvent::StreamStopped {
        stream_id,
        kind: StreamKind::Spectrum,
    };
    let _ = out_tx.send(text_event(&event)).await;
}

pub(super) fn spawn_iq_spectrum(
    stream_id: u16,
    fps: u16,
    bins: u16,
    mut rx: broadcast::Receiver<IqBlock>,
    out_tx: Outbox,
) -> tokio::task::JoinHandle<()> {
    let (fps, bins) = limits(fps, bins);
    tokio::spawn(async move {
        let mut ring = Ring::new(IQ_FFT);
        let mut rows = Rows::new(IQ_FFT, bins, false);
        let mut throttle = FrameThrottle::new(fps);
        let mut since = 0;
        loop {
            match rx.recv().await {
                Ok(block) => {
                    for &sample in block.samples.iter() {
                        ring.push(sample);
                        since += 1;
                        if since >= IQ_FFT / 2 && ring.ready() {
                            since = 0;
                            rows.accumulate(&ring);
                        }
                    }
                    if !rows.ready() || !throttle.admit(block.timestamp, block.sample_rate) {
                        continue;
                    }
                    let (db_min, db_max, floor_db, quant) = rows.take();
                    let frame = SpectrumFrame {
                        stream_id,
                        seq: block.seq,
                        timestamp: block.timestamp,
                        center_hz: block.center_hz,
                        span_hz: block.sample_rate,
                        db_min,
                        db_max,
                        floor_db,
                        bins: quant,
                    }
                    .encode();
                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    out_tx.dropped(count);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    stopped(stream_id, &out_tx).await;
                    break;
                }
            }
        }
    })
}

pub(super) fn spawn_pcm_spectrum(
    stream_id: u16,
    fps: u16,
    bins: u16,
    mut rx: broadcast::Receiver<PcmBlock>,
    out_tx: Outbox,
) -> tokio::task::JoinHandle<()> {
    let (fps, bins) = limits(fps, bins);
    tokio::spawn(async move {
        let mut ring = Ring::new(AUDIO_FFT);
        let mut rows = Rows::new(AUDIO_FFT, bins, true);
        let mut throttle = FrameThrottle::new(fps);
        let mut since = 0;
        let mut seq: u32 = 0;
        loop {
            match rx.recv().await {
                Ok(block) => {
                    let lanes = usize::from(block.channels).max(1);
                    let mut mono = |ring: &mut Ring, sample: f32| {
                        ring.push(Complex::new(sample, 0.0));
                        since += 1;
                        if since >= AUDIO_FFT / 2 && ring.ready() {
                            since = 0;
                            rows.accumulate(ring);
                        }
                    };
                    match &block.payload {
                        PcmPayload::Samples(samples) => {
                            for frame in samples.chunks(lanes) {
                                let sum: f32 = frame.iter().sum();
                                mono(&mut ring, sum / lanes as f32);
                            }
                        }
                        PcmPayload::Silence(frames) => {
                            for _ in 0..*frames {
                                mono(&mut ring, 0.0);
                            }
                        }
                    }
                    seq = seq.wrapping_add(1);
                    if !rows.ready() || !throttle.admit(block.start_frame, AUDIO_RATE) {
                        continue;
                    }
                    let (db_min, db_max, floor_db, quant) = rows.take();
                    let frame = SpectrumFrame {
                        stream_id,
                        seq,
                        timestamp: block.start_frame,
                        center_hz: f64::from(AUDIO_RATE) / 4.0,
                        span_hz: AUDIO_RATE / 2.0,
                        db_min,
                        db_max,
                        floor_db,
                        bins: quant,
                    }
                    .encode();
                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    out_tx.dropped(count);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    stopped(stream_id, &out_tx).await;
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_reads_back_oldest_first_once_full() {
        let mut ring = Ring::new(4);
        for value in 1..=6 {
            ring.push(Complex::new(value as f32, 0.0));
        }
        let mut out = vec![Complex::new(0.0, 0.0); 4];
        ring.oldest_first(&mut out);
        let seen: Vec<f32> = out.iter().map(|sample| sample.re).collect();
        assert_eq!(seen, [3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn a_ring_is_not_ready_before_it_holds_a_whole_window() {
        let mut ring = Ring::new(4);
        ring.push(Complex::new(1.0, 0.0));
        assert!(!ring.ready());
        for _ in 0..3 {
            ring.push(Complex::new(0.0, 0.0));
        }
        assert!(ring.ready());
    }

    #[test]
    fn a_tone_lands_in_the_bin_that_carries_its_frequency() {
        let mut ring = Ring::new(AUDIO_FFT);
        let bins = AUDIO_FFT / 2;
        for n in 0..AUDIO_FFT {
            let phase = std::f32::consts::TAU * 3000.0 * n as f32 / AUDIO_RATE;
            ring.push(Complex::new(phase.sin(), 0.0));
        }
        let mut rows = Rows::new(AUDIO_FFT, bins, true);
        rows.accumulate(&ring);
        let (_, _, _, quant) = rows.take();
        let peak = quant
            .iter()
            .enumerate()
            .max_by_key(|&(_, &level)| level)
            .map(|(bin, _)| bin)
            .expect("a quantized row has a peak");
        let hz = peak as f32 * (AUDIO_RATE / 2.0) / bins as f32;
        assert!((hz - 3000.0).abs() < 100.0, "peak landed at {hz} Hz");
    }

    #[test]
    fn a_row_carries_every_window_between_two_frames() {
        let bins = AUDIO_FFT / 2;
        let mut rows = Rows::new(AUDIO_FFT, bins, true);
        for hz in [3000.0, 9000.0] {
            let mut ring = Ring::new(AUDIO_FFT);
            for n in 0..AUDIO_FFT {
                let phase = std::f32::consts::TAU * hz * n as f32 / AUDIO_RATE;
                ring.push(Complex::new(phase.sin(), 0.0));
            }
            rows.accumulate(&ring);
        }
        let (db_min, db_max, _, quant) = rows.take();
        let level = |hz: f32| {
            let bin = (hz / (AUDIO_RATE / 2.0) * bins as f32).round() as usize;
            f32::from(quant[bin]) / 255.0 * (db_max - db_min) + db_min
        };
        let quiet = level(6000.0);
        assert!(
            level(3000.0) > quiet + 20.0 && level(9000.0) > quiet + 20.0,
            "both tones survive the average: {} and {} against {quiet}",
            level(3000.0),
            level(9000.0)
        );
    }

    #[test]
    fn limits_hold_the_client_inside_the_served_range() {
        assert_eq!(limits(0, 0), (1, MIN_BINS));
        assert_eq!(limits(240, 9000), (MAX_FPS, MAX_BINS));
    }
}
