use std::{sync::Arc, time::Duration};

use rtrb::Producer;
use zgui::{prelude::*, reactive::RenderEffect, view::IntervalHandle};

use crate::{
    audio::{CHANNELS, SAMPLE_RATE, TARGET_FRAMES, mixer::VoiceStats, output::Output},
    decoders::clip::{clock, decode_wav, to_stereo},
    store::Store,
};

const FEED_EVERY: Duration = Duration::from_millis(40);
const LEAD_FRAMES: usize = 2 * TARGET_FRAMES;
const TAIL_FRAMES: usize = 2 * TARGET_FRAMES;

#[derive(Clone, Debug, PartialEq)]
enum Clip {
    Idle,
    Loading,
    Playing { at_s: f64, length_s: f64 },
    Failed(String),
}

struct Playback {
    _output: Output,
    feed: Producer<f32>,
    stats: Arc<VoiceStats>,
    samples: Vec<f32>,
    written: usize,
    _timer: Option<IntervalHandle>,
}

impl Playback {
    fn queued_frames(&self) -> usize {
        let capacity = self.feed.buffer().capacity();
        (capacity - self.feed.slots()) / CHANNELS + self.stats.buffered_frames() as usize
    }

    fn total_samples(&self) -> usize {
        self.samples.len() + TAIL_FRAMES * CHANNELS
    }

    fn feed(&mut self) -> bool {
        let wanted = LEAD_FRAMES.saturating_sub(self.queued_frames()) * CHANNELS;
        let room = self.feed.slots() - self.feed.slots() % CHANNELS;
        let end = (self.written + wanted.min(room)).min(self.total_samples());
        while self.written < end {
            let sample = self.samples.get(self.written).copied().unwrap_or(0.0);
            if self.feed.push(sample).is_err() {
                break;
            }
            self.written += 1;
        }
        self.written < self.total_samples() || self.queued_frames() > 0
    }

    fn at_s(&self) -> f64 {
        let played = (self.written / CHANNELS).saturating_sub(self.queued_frames());
        played as f64 / f64::from(SAMPLE_RATE)
    }
}

#[derive(Clone, Copy)]
struct Player {
    store: Store,
    state: RwSignal<Clip>,
    playback: StoredValue<Option<Playback>, LocalStorage>,
}

impl Player {
    fn stop(self) {
        self.playback.set_value(None);
        self.state.set(Clip::Idle);
    }

    fn fail(self, error: impl ToString) {
        self.playback.set_value(None);
        self.state.set(Clip::Failed(error.to_string()));
    }

    fn start(self, url: String) {
        self.state.set(Clip::Loading);
        zgui::task::spawn_local(async move {
            match self.store.api().bytes(&url).await {
                Ok(bytes) => self.open(&bytes),
                Err(error) => self.fail(error),
            }
        });
    }

    fn open(self, bytes: &[u8]) {
        if self.state.get_untracked() != Clip::Loading {
            return;
        }
        let wav = match decode_wav(bytes) {
            Ok(wav) => wav,
            Err(error) => return self.fail(error),
        };
        let opened = Output::open().and_then(|mut output| {
            let stats = Arc::new(VoiceStats::with_gain(1.0));
            let (_, feed) = output.add(Arc::clone(&stats))?;
            Ok((output, feed, stats))
        });
        let (output, feed, stats) = match opened {
            Ok(opened) => opened,
            Err(error) => return self.fail(error),
        };
        let length_s = wav.seconds();
        let timer = set_interval(FEED_EVERY, move || self.tick(length_s));
        self.playback.set_value(Some(Playback {
            _output: output,
            feed,
            stats,
            samples: to_stereo(&wav, SAMPLE_RATE),
            written: 0,
            _timer: Some(timer),
        }));
        self.state.set(Clip::Playing {
            at_s: 0.0,
            length_s,
        });
        self.tick(length_s);
    }

    fn tick(self, length_s: f64) {
        let progress = self.playback.try_update_value(|held| {
            held.as_mut()
                .map(|playback| (playback.feed(), playback.at_s()))
        });
        match progress.flatten() {
            Some((true, at_s)) => self.state.set(Clip::Playing {
                at_s: at_s.min(length_s),
                length_s,
            }),
            Some((false, _)) => self.state.set(Clip::Idle),
            None => {}
        }
    }
}

pub fn clip_player(store: Store, url: String) -> impl IntoView {
    let player = Player {
        store,
        state: RwSignal::new(Clip::Idle),
        playback: StoredValue::new_local(None),
    };
    let release = RenderEffect::new(move |_| {
        if !matches!(player.state.get(), Clip::Playing { .. } | Clip::Loading) {
            player.playback.set_value(None);
        }
    });
    on_cleanup_local(move || {
        drop(release);
        player.playback.set_value(None);
    });
    let press = move |_: &mut EventCx<'_, events::Click>| match player.state.get_untracked() {
        Clip::Playing { .. } | Clip::Loading => player.stop(),
        Clip::Idle | Clip::Failed(_) => player.start(url.clone()),
    };
    let label = move || match player.state.get() {
        Clip::Playing { .. } | Clip::Loading => "Stop",
        Clip::Idle | Clip::Failed(_) => "Play",
    };
    let readout = move || match player.state.get() {
        Clip::Idle => None,
        Clip::Loading => Some("Loading…".to_owned()),
        Clip::Playing { at_s, length_s } => Some(format!("{} / {}", clock(at_s), clock(length_s))),
        Clip::Failed(error) => Some(format!("Cannot play: {error}")),
    };
    let failed = move || matches!(player.state.get(), Clip::Failed(_));
    view! {
        row(class = "dlog__player") {
            control(class = "btn", on:click:stop = press) {{label}}
            text(class = "dk-num", class:dk-danger = failed) {{readout}}
        }
    }
}
