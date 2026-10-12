use std::{sync::Arc, time::Duration};

use rtrb::Producer;
use zgui::prelude::*;

use crate::{
    audio::{CHANNELS, SAMPLE_RATE, TARGET_FRAMES, mixer::VoiceStats, output::Output},
    store::Store,
    ui::{
        kit_audio::format_clock,
        kit_shell::{hover, small_icon},
        library::wav::Wav,
        widgets::slide,
    },
};

const PLAY: &str = r#"<polygon points="6 3 20 12 6 21 6 3"/>"#;
const PAUSE: &str = r#"<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>"#;
const PUMP_MS: u64 = 20;
const PUMP: Duration = Duration::from_millis(PUMP_MS);
const PUMP_FRAMES: usize = SAMPLE_RATE as usize * PUMP_MS as usize / 1_000;
const AHEAD_FRAMES: usize = TARGET_FRAMES + PUMP_FRAMES / 2;

struct Voice {
    output: Output,
    id: u64,
    feed: Producer<f32>,
    stats: Arc<VoiceStats>,
}

impl Voice {
    fn open() -> anyhow::Result<Self> {
        let mut output = Output::open()?;
        let stats = Arc::new(VoiceStats::with_gain(1.0));
        let (id, feed) = output.add(stats.clone())?;
        Ok(Self {
            output,
            id,
            feed,
            stats,
        })
    }

    fn queued_frames(&self) -> usize {
        let in_feed = (self.feed.buffer().capacity() - self.feed.slots()) / CHANNELS;
        in_feed + self.stats.buffered_frames() as usize
    }

    fn flush(&self) {
        self.stats.restart();
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.output.remove(self.id);
    }
}

struct Clip {
    file: String,
    wav: Wav,
    cursor: f64,
    voice: Voice,
    block: Vec<f32>,
}

impl Clip {
    fn pump(&mut self) -> bool {
        let wanted = AHEAD_FRAMES.saturating_sub(self.voice.queued_frames());
        let room = self.voice.feed.slots() / CHANNELS;
        let frames = wanted.min(room);
        self.block.clear();
        self.wav
            .render(&mut self.cursor, SAMPLE_RATE, frames, &mut self.block);
        if let Err(error) = self.voice.feed.push_entire_slice(&self.block) {
            tracing::warn!(%error, "dropped a block of recording playback");
        }
        self.cursor < self.wav.frames() as f64 || self.voice.queued_frames() > 0
    }

    fn seconds(&self) -> f64 {
        self.cursor / f64::from(self.wav.sample_rate())
    }
}

#[derive(Clone, Copy)]
pub struct Player {
    store: Store,
    clip: StoredValue<Option<Clip>, LocalStorage>,
    playing: RwSignal<Option<String>>,
    paused: RwSignal<bool>,
    loading: RwSignal<Option<String>>,
    at_s: RwSignal<f64>,
    length_s: RwSignal<f64>,
}

impl Player {
    pub fn new(store: Store) -> Self {
        let player = Self {
            store,
            clip: StoredValue::new_local(None),
            playing: RwSignal::new(None),
            paused: RwSignal::new(false),
            loading: RwSignal::new(None),
            at_s: RwSignal::new(0.0),
            length_s: RwSignal::new(0.0),
        };
        let pumping = set_interval(PUMP, move || player.tick());
        on_cleanup_local(move || {
            drop(pumping);
            player.clip.try_update_value(|clip| *clip = None);
        });
        player
    }

    fn tick(self) {
        if self.paused.get_untracked() {
            return;
        }
        let state = self
            .clip
            .try_update_value(|clip| clip.as_mut().map(|clip| (clip.pump(), clip.seconds())));
        match state.flatten() {
            Some((true, at)) => {
                self.at_s.set(at);
            }
            Some((false, _)) => self.stop(),
            None => {}
        }
    }

    pub fn stop(self) {
        self.clip.update_value(|clip| *clip = None);
        self.playing.set(None);
        self.paused.set(false);
        self.at_s.set(0.0);
    }

    fn toggle(self, file: &str) {
        if self.playing.get_untracked().as_deref() == Some(file) {
            let pausing = !self.paused.get_untracked();
            if pausing {
                self.clip.with_value(|clip| {
                    if let Some(clip) = clip {
                        clip.voice.flush();
                    }
                });
            }
            self.paused.set(pausing);
            return;
        }
        self.load(file.to_owned());
    }

    fn load(self, file: String) {
        self.stop();
        self.loading.set(Some(file.clone()));
        zgui::task::spawn_local(async move {
            let fetched = self
                .store
                .api()
                .bytes(&format!("/api/audiorecordings/{file}"))
                .await;
            self.loading.try_set(None);
            match fetched.and_then(Wav::parse) {
                Ok(wav) => self.start(file, wav),
                Err(error) => self.store.fail("Cannot play the recording", &error),
            }
        });
    }

    fn start(self, file: String, wav: Wav) {
        if self.playing.try_get_untracked().is_none() {
            return;
        }
        let voice = match Voice::open() {
            Ok(voice) => voice,
            Err(error) => {
                self.store.fail("Cannot open the audio output", &error);
                return;
            }
        };
        self.length_s.set(wav.duration_s());
        self.at_s.set(0.0);
        self.playing.set(Some(file.clone()));
        self.clip.set_value(Some(Clip {
            file,
            wav,
            cursor: 0.0,
            voice,
            block: Vec::with_capacity(AHEAD_FRAMES * CHANNELS),
        }));
    }

    fn seek(self, seconds: f64) {
        self.clip.update_value(|clip| {
            if let Some(clip) = clip {
                let rate = f64::from(clip.wav.sample_rate());
                clip.cursor = (seconds * rate).clamp(0.0, clip.wav.frames() as f64);
                clip.voice.flush();
            }
        });
        self.at_s.set(seconds);
    }

    pub fn stop_if(self, file: &str) {
        if self.playing.get_untracked().as_deref() == Some(file) {
            self.stop();
        }
    }

    fn is(self, file: &str) -> bool {
        self.playing.get().as_deref() == Some(file)
            && self
                .clip
                .with_value(|clip| clip.as_ref().is_some_and(|clip| clip.file == file))
    }
}

pub fn play_button(player: Player, file: String) -> impl IntoView {
    move || {
        let label = if player.loading.get().as_deref() == Some(file.as_str()) {
            "Loading"
        } else if player.is(&file) && !player.paused.get() {
            "Pause"
        } else {
            "Play"
        };
        let glyph = if label == "Pause" { PAUSE } else { PLAY };
        let pressed = file.clone();
        hover(
            label,
            view! {
                control(
                    class = "sk-iconbtn sm",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = label,
                    on:click:stop = move |_| player.toggle(&pressed)
                ) {
                    {small_icon(glyph)}
                }
            },
        )
    }
}

pub fn transport(player: Player, file: String) -> impl IntoView {
    move || {
        player.is(&file).then(|| {
            let read = move |at: f64| {
                format!(
                    "{} / {}",
                    format_clock(at),
                    format_clock(player.length_s.get_untracked())
                )
            };
            let length = player.length_s.get_untracked().max(f64::EPSILON);
            AnyView::new(view! {
                row(class = "sk-toolbar") {
                    {slide(player.at_s.into(), 0.0, length, read, move |at| player.seek(at))}
                }
            })
        })
    }
}
