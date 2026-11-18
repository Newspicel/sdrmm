//! Carrier: reports the strongest unmodulated carrier in its passband, written like a built-in
//! decoder: a settings struct, a channel, an event kind with its presenter, a feed and the
//! [`Native`] builder that declares them.

use std::{
    sync::{Arc, LazyLock},
    time::{SystemTime, UNIX_EPOCH},
};

use sdrmm_plugin::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, Complex, Native, NativeChannel,
    decoder::{check_input_rate, settings_of},
    kernels::Fft,
    wire::{
        Capability, ChannelDescriptor, ChannelHealth, ChannelSettings, ChannelStatus, DecoderEvent,
        DecoderFamily, DecoderSettings, EventContract, EventKind, FeedEncoder, FeedSpec,
        FilterSpec, IdentSignal, KindSet, KindSpec, KindView, NumberSpec, PositionFix,
        SettingField,
        patch::Hue,
        present::{EventPayload, Presentation, Unit, fields, number},
    },
};
use serde::{Deserialize, Serialize};

const TYPE_ID: &str = "carrier";
pub(crate) const INPUT_RATE_HZ: f64 = 48_000.0;
const FFT_LEN: usize = 1_024;
const FILTER_TAPS: usize = 65;
pub(crate) const KIND: EventKind = EventKind::named("carrier");
/// FFT blocks between two health reports.
const STATUS_EVERY: u32 = 16;
/// Bins either side of the receiver's LO artifact that are never taken for a carrier.
const LO_GUARD_BINS: i64 = 2;
const MIN_WIDTH_HZ: f64 = 1_000.0;
const MAX_WIDTH_HZ: f64 = 20_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CarrierSettings {
    pub bandwidth_hz: f64,
    pub threshold_db: f64,
}

impl Default for CarrierSettings {
    fn default() -> Self {
        Self {
            bandwidth_hz: 10_000.0,
            threshold_db: 20.0,
        }
    }
}

impl DecoderSettings for CarrierSettings {
    const TYPE_ID: &'static str = TYPE_ID;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierSeen {
    pub offset_hz: f64,
    pub snr_db: f64,
    /// When the block it was heard in was captured, in Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heard_at_ms: Option<i64>,
    /// Where the receiver stood.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<PositionFix>,
}

impl EventContract for CarrierSeen {
    const KIND: EventKind = KIND;
}

impl EventPayload for CarrierSeen {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                (
                    "offset",
                    "Offset",
                    number(Some(self.offset_hz), Unit::Hz, 0),
                ),
                ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 1)),
            ]),
            ..Presentation::of(format!("Carrier {:+.0} Hz", self.offset_hz))
        }
    }
}

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: TYPE_ID.to_owned(),
    name: "Carrier".to_owned(),
    summary: "Strongest unmodulated carrier".to_owned(),
    family: DecoderFamily::UTILITY,
    bandwidth_hz: MAX_WIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![KIND],
    ..ChannelDescriptor::default()
});

/// The decoder as the plugin hands it over, declared like a built-in one.
pub(crate) fn decoder() -> Native<CarrierSettings> {
    Native::new::<Carrier>(band)
        .filter(|settings| Ok(filter(settings)))
        .form(form)
        .kinds(kinds)
        .feeds(feeds)
        .tune(tune)
}

fn band(settings: &CarrierSettings) -> (f64, f64) {
    let half = settings.bandwidth_hz / 2.0;
    (-half, half)
}

fn filter(settings: &CarrierSettings) -> FilterSpec {
    FilterSpec::lowpass(FILTER_TAPS, settings.bandwidth_hz / 2.0)
}

fn form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "bandwidth_hz",
            "Width",
            NumberSpec::hz(MIN_WIDTH_HZ, MAX_WIDTH_HZ, 500.0),
        )
        .title("Passband searched for a carrier"),
        SettingField::new(
            "threshold_db",
            "Threshold",
            NumberSpec::new(3.0, 60.0, 1.0).unit("dB"),
        )
        .title("SNR a carrier needs to be reported"),
    ]
}

fn kinds() -> Vec<KindSpec> {
    vec![
        KindSpec::of::<CarrierSeen>("Carrier", "Carriers", Hue::Teal)
            .caps(&[Capability::Log])
            .view(KindView::meter("snr", 0.0, 60.0))
            .view(KindView::trend(&["offset"], 300))
            .build(),
    ]
}

/// A line of text per carrier, for anything that reads a socket.
struct CarrierText;

impl FeedEncoder for CarrierText {
    fn encode(&self, event: &DecoderEvent) -> Result<Vec<u8>, String> {
        let seen = event
            .read::<CarrierSeen>()
            .ok_or_else(|| format!("carrier text carries carriers, not {}", event.kind))?;
        Ok(format!("{:+.0} Hz {:.1} dB\n", seen.offset_hz, seen.snr_db).into_bytes())
    }
}

fn feeds() -> Vec<FeedSpec> {
    vec![FeedSpec {
        format: "carrier-text".to_owned(),
        name: "Carrier text".to_owned(),
        default_address: "127.0.0.1:30200".to_owned(),
        kinds: KindSet::EMPTY.with(KIND),
        heartbeat: Vec::new(),
        encoder: Arc::new(CarrierText),
    }]
}

/// Opens the passband to twice the identified signal's width and centres on it.
fn tune(signal: &IdentSignal, settings: &mut CarrierSettings) -> f64 {
    settings.bandwidth_hz = (2.0 * signal.bandwidth_hz).clamp(MIN_WIDTH_HZ, MAX_WIDTH_HZ);
    0.0
}

pub struct Carrier {
    settings: CarrierSettings,
    fft: Fft,
    block: Vec<Complex<f32>>,
    power: Vec<f64>,
    sorted: Vec<f64>,
    held: Option<usize>,
    blocks: u32,
    lo_bin: Option<i64>,
    captured: Option<SystemTime>,
    receiver: Option<PositionFix>,
}

impl NativeChannel for Carrier {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            settings: settings_of(&settings)?,
            fft: Fft::forward(FFT_LEN).map_err(ChannelError::Unsupported)?,
            block: Vec::with_capacity(FFT_LEN),
            power: Vec::with_capacity(FFT_LEN),
            sorted: Vec::with_capacity(FFT_LEN),
            held: None,
            blocks: 0,
            lo_bin: None,
            captured: None,
            receiver: None,
        })
    }
}

impl Carrier {
    fn report(&mut self, health: ChannelHealth, out: &mut ChannelOutputs) {
        self.blocks = self.blocks.wrapping_add(1);
        if self.blocks.is_multiple_of(STATUS_EVERY) {
            out.status = Some(ChannelStatus {
                health,
                ..ChannelStatus::default()
            });
        }
    }

    fn near_lo(&self, bin: usize) -> bool {
        self.lo_bin
            .is_some_and(|lo| (signed_bin(bin) - lo).abs() <= LO_GUARD_BINS)
    }

    fn peak(&self) -> Option<(usize, f64)> {
        self.power
            .iter()
            .copied()
            .enumerate()
            .filter(|&(bin, _)| !self.near_lo(bin))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn analyse(&mut self, out: &mut ChannelOutputs) {
        self.fft.run(&mut self.block);
        self.power.clear();
        self.power
            .extend(self.block.iter().map(|bin| f64::from(bin.norm_sqr())));
        self.block.clear();
        let Some((peak, top)) = self.peak() else {
            return;
        };
        self.sorted.clone_from(&self.power);
        self.sorted.sort_by(f64::total_cmp);
        let floor = self.sorted[self.sorted.len() / 2].max(f64::MIN_POSITIVE);
        let snr_db = 10.0 * (top / floor).log10();
        let offset_hz = signed_bin(peak) as f64 * INPUT_RATE_HZ / FFT_LEN as f64;
        let inside = offset_hz.abs() <= self.settings.bandwidth_hz / 2.0;
        let locked = snr_db >= self.settings.threshold_db && inside;
        let health = ChannelHealth::default().lock(locked).snr(snr_db as f32);
        let health = if locked {
            health.freq_error(offset_hz)
        } else {
            health
        };
        self.report(health, out);
        if !locked {
            self.held = None;
            return;
        }
        if self.held != Some(peak) {
            self.held = Some(peak);
            out.events.push(DecoderEvent::of(&CarrierSeen {
                offset_hz,
                snr_db,
                heard_at_ms: self.captured.and_then(unix_ms),
                receiver: self.receiver.clone(),
            }));
        }
    }
}

fn signed_bin(bin: usize) -> i64 {
    let bin = bin as i64;
    if bin >= FFT_LEN as i64 / 2 {
        bin - FFT_LEN as i64
    } else {
        bin
    }
}

fn unix_ms(time: SystemTime) -> Option<i64> {
    let since = time.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

impl Channel for Carrier {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.settings = settings_of(&settings)?;
        Ok(())
    }

    fn position_changed(&mut self, fix: Option<&PositionFix>) {
        self.receiver = fix.cloned();
    }

    fn captured_at(&mut self, time: SystemTime) {
        if self.block.is_empty() {
            self.captured = Some(time);
        }
    }

    fn lo_artifact_at(&mut self, offset_hz: Option<f64>) {
        self.lo_bin = offset_hz.map(|hz| (hz * FFT_LEN as f64 / INPUT_RATE_HZ).round() as i64);
    }

    /// Squelched silence holds no carrier, so the channel waits for the squelch to open.
    fn needs_gated_input(&self) -> bool {
        false
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        for &sample in iq {
            self.block.push(sample);
            if self.block.len() == FFT_LEN {
                self.analyse(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_plugin::{Decoder, wire::ChannelParams};

    use super::*;

    fn tone(offset_hz: f64, samples: usize) -> Vec<Complex<f32>> {
        (0..samples)
            .map(|n| {
                let phase = std::f64::consts::TAU * offset_hz * n as f64 / INPUT_RATE_HZ;
                Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect()
    }

    fn open(settings: &CarrierSettings) -> Result<Carrier, ChannelError> {
        let settings = ChannelSettings::new(
            1e6,
            ChannelParams::new(TYPE_ID, settings).expect("settings"),
        );
        Carrier::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings,
        )
    }

    fn seen(out: &ChannelOutputs) -> Vec<CarrierSeen> {
        out.events
            .iter()
            .filter_map(DecoderEvent::read::<CarrierSeen>)
            .collect()
    }

    #[test]
    fn a_steady_tone_is_reported_once_near_its_offset() {
        let mut carrier = open(&CarrierSettings::default()).expect("opens at 48 kHz");
        let mut out = ChannelOutputs::default();
        carrier.captured_at(UNIX_EPOCH + std::time::Duration::from_secs(2));
        carrier.process(&tone(1_500.0, FFT_LEN * 4), &mut out);
        let seen = seen(&out);
        assert_eq!(seen.len(), 1);
        assert!((seen[0].offset_hz - 1_500.0).abs() < 50.0, "{seen:?}");
        assert!(seen[0].snr_db > 20.0);
        assert_eq!(seen[0].heard_at_ms, Some(2_000));
    }

    #[test]
    fn a_tone_outside_the_passband_is_ignored() {
        let settings = CarrierSettings {
            bandwidth_hz: 2_000.0,
            ..CarrierSettings::default()
        };
        let mut carrier = open(&settings).expect("opens");
        let mut out = ChannelOutputs::default();
        carrier.process(&tone(5_000.0, FFT_LEN * 2), &mut out);
        assert!(out.events.is_empty());
    }

    #[test]
    fn the_receivers_own_lo_artifact_is_no_carrier() {
        let mut carrier = open(&CarrierSettings::default()).expect("opens");
        carrier.lo_artifact_at(Some(1_500.0));
        let mut out = ChannelOutputs::default();
        carrier.process(&tone(1_500.0, FFT_LEN * 2), &mut out);
        assert!(out.events.is_empty(), "{:?}", out.events);
    }

    #[test]
    fn a_held_tone_reports_lock_and_its_offset_in_the_health_footer() {
        let mut carrier = open(&CarrierSettings::default()).expect("opens");
        let mut out = ChannelOutputs::default();
        carrier.process(&tone(1_500.0, FFT_LEN * STATUS_EVERY as usize), &mut out);
        let health = out.status.expect("a status").health;
        assert_eq!(health.lock, Some(true));
        assert!((health.freq_error_hz.expect("offset") - 1_500.0).abs() < 50.0);
        assert!(health.snr_db.expect("snr") > 20.0);
    }

    #[test]
    fn the_wrong_input_rate_is_refused() {
        let settings = ChannelSettings::new(1e6, ChannelParams::bare(TYPE_ID));
        assert!(Carrier::new(ChannelCtx { input_rate: 96e3 }, settings).is_err());
    }

    #[test]
    fn the_filter_follows_the_width_and_tuning_follows_the_signal() {
        let decoder: Box<dyn Decoder> = Box::new(decoder());
        let params = ChannelParams::new(
            TYPE_ID,
            &CarrierSettings {
                bandwidth_hz: 4_000.0,
                ..CarrierSettings::default()
            },
        )
        .expect("params");
        assert_eq!(
            decoder.filter(&params).expect("a filter"),
            FilterSpec::lowpass(FILTER_TAPS, 2_000.0)
        );
        let signal = IdentSignal {
            bandwidth_hz: 3_000.0,
            ..IdentSignal::default()
        };
        let tuned = decoder.tune(&params, &signal).expect("tunes");
        assert_eq!(tuned.params.settings["bandwidth_hz"], 6_000.0);
    }

    #[test]
    fn the_feed_writes_a_line_per_carrier() {
        let event = DecoderEvent::of(&CarrierSeen {
            offset_hz: 1_200.0,
            snr_db: 30.0,
            heard_at_ms: None,
            receiver: None,
        });
        let line = feeds()[0].encoder.encode(&event).expect("encodes");
        assert_eq!(line, b"+1200 Hz 30.0 dB\n");
    }
}
