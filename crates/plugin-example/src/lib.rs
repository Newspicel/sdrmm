//! Carrier: reports the strongest unmodulated carrier in its passband.
//!
//! A template for decoder plugins: a settings form, one event kind with a presenter and view
//! hints, the health footer, a host FFT doing the heavy lifting, a transmitter ([`Beacon`]) and
//! an array processor ([`LaneMeter`]).
//! Build it in this folder with `cargo build --target wasm32-unknown-unknown --release` and drop
//! the `.wasm` into the server's plugins directory.

mod beacon;
pub mod lane_meter;

pub use beacon::Beacon;
pub use lane_meter::LaneMeter;
use sdrmm_plugin::{
    Complex, Outputs, Plugin,
    kernels::Fft,
    serde_json::Value,
    wire::{
        Capability, ChannelDescriptor, ChannelHealth, ChannelStatus, DecoderFamily, EventContract,
        EventKind, EventKindEntry, KindView, NumberSpec, SettingField,
        patch::Hue,
        plugin::{ABI_VERSION, PluginLayout, PluginManifest},
        present::{Presentation, Unit, fields, number},
    },
};
use serde::{Deserialize, Serialize};

const TYPE_ID: &str = "carrier";
const INPUT_RATE_HZ: f64 = 48_000.0;
const FFT_LEN: usize = 1_024;
const KIND: EventKind = EventKind::named("carrier");
/// FFT blocks between two health reports.
const STATUS_EVERY: u32 = 16;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierSeen {
    pub offset_hz: f64,
    pub snr_db: f64,
}

impl EventContract for CarrierSeen {
    const KIND: EventKind = KIND;
}

pub struct Carrier {
    rate: f64,
    settings: CarrierSettings,
    fft: Fft,
    block: Vec<Complex<f32>>,
    held: Option<i64>,
    blocks: u32,
}

impl Carrier {
    fn report(&mut self, health: ChannelHealth, out: &mut Outputs) {
        self.blocks = self.blocks.wrapping_add(1);
        if self.blocks.is_multiple_of(STATUS_EVERY) {
            out.status = Some(ChannelStatus {
                health,
                ..ChannelStatus::default()
            });
        }
    }

    fn analyse(&mut self, out: &mut Outputs) {
        self.fft.run(&mut self.block);
        let power: Vec<f64> = self
            .block
            .iter()
            .map(|bin| f64::from(bin.norm_sqr()))
            .collect();
        self.block.clear();
        let Some((peak, &top)) = power.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) else {
            return;
        };
        let mut sorted = power.clone();
        sorted.sort_by(f64::total_cmp);
        let floor = sorted[sorted.len() / 2].max(f64::MIN_POSITIVE);
        let snr_db = 10.0 * (top / floor).log10();
        let offset_hz = bin_offset(peak, self.rate);
        let inside = offset_hz.abs() <= self.settings.bandwidth_hz / 2.0;
        let locked = snr_db >= self.settings.threshold_db && inside;
        let health = ChannelHealth::default().lock(locked).snr(snr_db as f32);
        self.report(
            if locked {
                health.freq_error(offset_hz)
            } else {
                health
            },
            out,
        );
        if !locked {
            self.held = None;
            return;
        }
        let bin = i64::try_from(peak).unwrap_or(i64::MAX);
        if self.held != Some(bin) {
            self.held = Some(bin);
            out.emit(&CarrierSeen { offset_hz, snr_db });
        }
    }
}

fn bin_offset(bin: usize, rate: f64) -> f64 {
    let signed = if bin >= FFT_LEN / 2 {
        bin as f64 - FFT_LEN as f64
    } else {
        bin as f64
    };
    signed * rate / FFT_LEN as f64
}

impl Plugin for Carrier {
    type Settings = CarrierSettings;

    fn manifest() -> PluginManifest {
        PluginManifest {
            abi: ABI_VERSION,
            descriptor: ChannelDescriptor {
                type_id: TYPE_ID.to_owned(),
                name: "Carrier".to_owned(),
                summary: "Strongest unmodulated carrier".to_owned(),
                family: DecoderFamily::UTILITY,
                bandwidth_hz: 20_000.0,
                input_rate_hz: INPUT_RATE_HZ,
                has_audio: false,
                emits: vec![KIND],
                can_transmit: true,
                ..ChannelDescriptor::default()
            },
            form: vec![
                SettingField::new(
                    "bandwidth_hz",
                    "Width",
                    NumberSpec::hz(1_000.0, 20_000.0, 500.0),
                )
                .title("Passband searched for a carrier"),
                SettingField::new(
                    "threshold_db",
                    "Threshold",
                    NumberSpec::new(3.0, 60.0, 1.0).unit("dB"),
                )
                .title("SNR a carrier needs to be reported"),
            ],
            kinds: vec![
                EventKindEntry::new(KIND, "Carrier", "Carriers", Hue::Teal)
                    .caps(&[Capability::Log])
                    .view(KindView::meter("snr", 0.0, 60.0))
                    .view(KindView::trend(&["offset"], 300)),
            ],
            home_hz: None,
            processor: Some(lane_meter::kind()),
        }
    }

    fn layout(settings: &CarrierSettings) -> PluginLayout {
        let half = settings.bandwidth_hz / 2.0;
        PluginLayout {
            input_rate_hz: INPUT_RATE_HZ,
            band: (-half, half),
            audio_channels: 1,
        }
    }

    fn open(input_rate_hz: f64, settings: CarrierSettings) -> Result<Self, String> {
        if input_rate_hz != INPUT_RATE_HZ {
            return Err(format!(
                "carrier expects {INPUT_RATE_HZ} Hz input, got {input_rate_hz}"
            ));
        }
        Ok(Self {
            rate: input_rate_hz,
            settings,
            fft: Fft::forward(FFT_LEN)?,
            block: Vec::with_capacity(FFT_LEN),
            held: None,
            blocks: 0,
        })
    }

    fn apply(&mut self, settings: CarrierSettings) -> Result<(), String> {
        self.settings = settings;
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut Outputs) {
        for &sample in iq {
            self.block.push(sample);
            if self.block.len() == FFT_LEN {
                self.analyse(out);
            }
        }
    }

    fn present(_kind: &EventKind, data: &Value) -> Result<Presentation, String> {
        let seen: CarrierSeen = sdrmm_plugin::serde_json::from_value(data.clone())
            .map_err(|error| error.to_string())?;
        Ok(Presentation {
            fields: fields(&[
                (
                    "offset",
                    "Offset",
                    number(Some(seen.offset_hz), Unit::Hz, 0),
                ),
                ("snr", "SNR", number(Some(seen.snr_db), Unit::Db, 1)),
            ]),
            ..Presentation::of(format!("Carrier {:+.0} Hz", seen.offset_hz))
        })
    }
}

sdrmm_plugin::export!(Carrier);
sdrmm_plugin::export_transmit!(Beacon);
sdrmm_plugin::export_processor!(LaneMeter);

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(offset_hz: f64, samples: usize) -> Vec<Complex<f32>> {
        (0..samples)
            .map(|n| {
                let phase = 2.0 * std::f64::consts::PI * offset_hz * n as f64 / INPUT_RATE_HZ;
                Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect()
    }

    #[test]
    fn a_steady_tone_is_reported_once_near_its_offset() {
        let mut carrier =
            Carrier::open(INPUT_RATE_HZ, CarrierSettings::default()).expect("opens at 48 kHz");
        let mut out = Outputs::default();
        carrier.process(&tone(1_500.0, FFT_LEN * 4), &mut out);
        assert_eq!(out.events.len(), 1);
        let seen: CarrierSeen = out.events[0].read().expect("a carrier event");
        assert!(
            (seen.offset_hz - 1_500.0).abs() < 50.0,
            "{}",
            seen.offset_hz
        );
        assert!(seen.snr_db > 20.0);
    }

    #[test]
    fn a_tone_outside_the_passband_is_ignored() {
        let settings = CarrierSettings {
            bandwidth_hz: 2_000.0,
            ..CarrierSettings::default()
        };
        let mut carrier = Carrier::open(INPUT_RATE_HZ, settings).expect("opens");
        let mut out = Outputs::default();
        carrier.process(&tone(5_000.0, FFT_LEN * 2), &mut out);
        assert!(out.events.is_empty());
    }

    #[test]
    fn a_held_tone_reports_lock_and_its_offset_in_the_health_footer() {
        let mut carrier = Carrier::open(INPUT_RATE_HZ, CarrierSettings::default()).expect("opens");
        let mut out = Outputs::default();
        carrier.process(&tone(1_500.0, FFT_LEN * STATUS_EVERY as usize), &mut out);
        let health = out.status.expect("a status").health;
        assert_eq!(health.lock, Some(true));
        assert!((health.freq_error_hz.expect("offset") - 1_500.0).abs() < 50.0);
        assert!(health.snr_db.expect("snr") > 20.0);
    }

    #[test]
    fn the_wrong_input_rate_is_refused() {
        assert!(Carrier::open(96_000.0, CarrierSettings::default()).is_err());
    }

    #[test]
    fn an_event_presents_its_offset() {
        let data = sdrmm_plugin::serde_json::json!({ "offset_hz": -250.0, "snr_db": 31.5 });
        let view = Carrier::present(&KIND, &data).expect("presents");
        assert_eq!(view.summary, "Carrier -250 Hz");
    }
}
