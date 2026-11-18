//! The harness every decoder's positive fixture runs in. Each decoder keeps its fixture, what it
//! must read and how to make the signal, in its own `fixture` module.

use num_complex::Complex;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelFront, ChannelOutputs, Decoder, create, input_rate,
};
use sdrmm_decoder_test_support::scene::Scene;
use sdrmm_decoder_test_support::{complex_noise, settings};
use sdrmm_wire::{ChannelParams, ChannelSettings, DecoderEvent, EventKind};

const CHUNK: usize = 4_096;
const PLACE_TOLERANCE_DEG: f64 = 0.001;
const VOICE_RATE: f64 = 48_000.0;

#[derive(Default)]
pub(crate) struct Heard {
    kinds: Vec<EventKind>,
    events: Vec<DecoderEvent>,
    audio_samples: usize,
    pictures: usize,
}

impl Heard {
    pub(crate) fn from_events(events: &[DecoderEvent]) -> Self {
        Self {
            kinds: events.iter().map(DecoderEvent::kind).collect(),
            events: events.to_vec(),
            ..Self::default()
        }
    }

    fn take(&mut self, out: &mut ChannelOutputs) {
        for event in out.events.drain(..) {
            self.kinds.push(event.kind());
            self.events.push(event);
        }
        self.audio_samples += out.audio_pcm.len();
        self.pictures += out.video.len() + out.images.len();
    }

    fn rendered(&self) -> String {
        let events = self.events.iter().map(|event| {
            let view = event.present();
            let fields = view
                .fields
                .iter()
                .map(|field| format!("{}={:?}", field.label, field.value))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "{} | {} | {} | {} | {} | {}",
                serde_json::to_string(event).unwrap_or_default(),
                view.summary,
                view.headline.unwrap_or_default(),
                view.entity.unwrap_or_default(),
                view.text.unwrap_or_default(),
                fields
            )
        });
        events.collect::<Vec<_>>().join("\n")
    }

    fn places(&self) -> Vec<(f64, f64)> {
        self.events
            .iter()
            .filter_map(|event| event.present().place)
            .map(|place| (place.at.lat, place.at.lon))
            .collect()
    }
}

/// What a decoder must read from its positive fixture, and how to hear that fixture.
pub(crate) struct Positive {
    carries: &'static [&'static str],
    at: Option<(f64, f64)>,
    audio_samples: usize,
    pictures: usize,
    heard: fn() -> Heard,
}

/// What a fixture must carry, as text its rendered events contain.
pub(crate) const fn reads(carries: &'static [&'static str]) -> Expected {
    Expected {
        carries,
        at: None,
        audio_samples: 0,
        pictures: 0,
    }
}

pub(crate) struct Expected {
    carries: &'static [&'static str],
    at: Option<(f64, f64)>,
    audio_samples: usize,
    pictures: usize,
}

impl Expected {
    pub(crate) const fn at(self, lat: f64, lon: f64) -> Self {
        Self {
            at: Some((lat, lon)),
            ..self
        }
    }

    pub(crate) const fn audio(self, audio_samples: usize) -> Self {
        Self {
            audio_samples,
            ..self
        }
    }

    pub(crate) const fn pictures(self, pictures: usize) -> Self {
        Self { pictures, ..self }
    }

    /// The positive this expectation makes of the fixture `heard` runs.
    pub(crate) const fn hearing(self, heard: fn() -> Heard) -> Positive {
        Positive {
            carries: self.carries,
            at: self.at,
            audio_samples: self.audio_samples,
            pictures: self.pictures,
            heard,
        }
    }
}

impl Positive {
    pub(crate) fn proves_a_decode(&self) -> bool {
        !self.carries.is_empty() || self.audio_samples > 0 || self.pictures > 0
    }
}

pub(crate) struct Fixture {
    settings: ChannelSettings,
    iq: Vec<Complex<f32>>,
}

impl Fixture {
    pub(crate) fn new(params: ChannelParams, iq: Vec<Complex<f32>>) -> Self {
        Self {
            settings: settings(params),
            iq,
        }
    }

    pub(crate) fn tuned(mut self, frequency_hz: f64) -> Self {
        self.settings.frequency_hz = frequency_hz;
        self
    }

    pub(crate) fn rate(params: &ChannelParams) -> f64 {
        input_rate(params)
    }

    pub(crate) fn heard(self) -> Heard {
        let rate = input_rate(&self.settings.params);
        let mut front = ChannelFront::new(&self.settings.params, rate, 0.0).expect("front");
        let mut channel = create(ChannelCtx { input_rate: rate }, &self.settings).expect("builds");
        let mut filtered = Vec::new();
        let mut out = ChannelOutputs::default();
        let mut heard = Heard::default();
        for block in self.iq.chunks(CHUNK) {
            front.process(block, &mut filtered);
            out.reset();
            channel.process(&filtered, &mut out);
            heard.take(&mut out);
        }
        heard
    }
}

pub(crate) fn with_tail(mut iq: Vec<Complex<f32>>, rate: f64, seconds: f64) -> Vec<Complex<f32>> {
    iq.extend(sdrmm_modem::synth::silence((seconds * rate) as usize));
    iq
}

pub(crate) fn with_lead(iq: Vec<Complex<f32>>, rate: f64, seconds: f64) -> Vec<Complex<f32>> {
    let mut led = sdrmm_modem::synth::silence((seconds * rate) as usize);
    led.extend(iq);
    led
}

pub(crate) fn ci16_le(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0,
                f32::from(i16::from_le_bytes([s[2], s[3]])) / 32_768.0,
            )
        })
        .collect()
}

pub(crate) fn cu8(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[i, q]| {
            Complex::new(
                (f32::from(i) - 127.5) / 127.5,
                (f32::from(q) - 127.5) / 127.5,
            )
        })
        .collect()
}

/// A keyed-up voice transmission: a little noise before `iq`, then half a second of silence.
pub(crate) fn keyed_after_noise(iq: Vec<Complex<f32>>) -> Vec<Complex<f32>> {
    let mut keyed = complex_noise(1, 0.01, 19_200);
    keyed.extend(iq);
    with_tail(keyed, VOICE_RATE, 0.5)
}

/// Runs `decoder`'s positive fixture and checks it read what it must, in kinds it declares.
pub(crate) fn check(decoder: &dyn Decoder, expected: &Positive) {
    let descriptor = decoder.descriptor();
    let type_id = &descriptor.type_id;
    let heard = (expected.heard)();
    if !expected.carries.is_empty() {
        assert!(
            !heard.kinds.is_empty(),
            "{type_id} produced nothing from its fixture"
        );
    }
    for kind in &heard.kinds {
        assert!(
            descriptor.emits.contains(kind),
            "{type_id} emitted {kind} but declares {:?}",
            descriptor.emits
        );
    }
    let rendered = heard.rendered();
    for carry in expected.carries {
        assert!(
            rendered.contains(carry),
            "{type_id} fixture read {rendered} and never carried {carry}"
        );
    }
    if let Some((lat, lon)) = expected.at {
        let places = heard.places();
        assert!(
            places.iter().any(|&(heard_lat, heard_lon)| {
                (heard_lat - lat).abs() < PLACE_TOLERANCE_DEG
                    && (heard_lon - lon).abs() < PLACE_TOLERANCE_DEG
            }),
            "{type_id} fixture placed {places:?} and never {lat}, {lon}"
        );
    }
    assert!(
        heard.audio_samples >= expected.audio_samples,
        "{type_id} fixture gave {} audio samples, short of {}",
        heard.audio_samples,
        expected.audio_samples
    );
    assert!(
        heard.pictures >= expected.pictures,
        "{type_id} fixture gave {} pictures, short of {}",
        heard.pictures,
        expected.pictures
    );
}

pub(crate) fn scene_heard(params: ChannelParams, frequency_hz: f64, scene: &Scene) -> Heard {
    let rate = input_rate(&params);
    let mut tuned = settings(params);
    tuned.frequency_hz = frequency_hz;
    let mut channel: Box<dyn Channel> =
        create(ChannelCtx { input_rate: rate }, &tuned).expect("builds");
    Heard::from_events(&scene.run(1.01, channel.as_mut()).events)
}
