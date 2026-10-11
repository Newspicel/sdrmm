mod pacer;

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use sdrmm_wire::{
    AudioRoute,
    frame::{FrameKind, VideoData, VideoFrame},
    ws::ClientCommand,
};
use zgui::{
    prelude::*,
    view::{TimeoutHandle, Timers},
};

use self::pacer::{Pacer, clamp_delay_ms};
use crate::{
    audio::{Audio, use_audio},
    binding,
    bus::Source,
    store::Store,
    ui::{
        kit_audio::{self, Stat, Tone},
        kit_raster::{Raster, Scene, raster_view},
    },
};

const DISPLAY_ASPECT: f32 = 4.0 / 3.0;
const STALE: Duration = Duration::from_secs(2);
const STALE_CHECK: Duration = Duration::from_millis(500);
const RELEASE_GRACE: Duration = Duration::from_secs(5);
const PICTURE_HINT: &str = "Decoded picture size and sync";

type Key = (u32, u32);

thread_local! {
    static GRACE: RefCell<HashMap<Key, TimeoutHandle>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSignal {
    pub width: u16,
    pub height: u16,
    pub live: bool,
}

type Signals = RwSignal<HashMap<String, VideoSignal>>;

#[must_use]
pub fn rgba_of(data: VideoData<'_>, width: u16, height: u16) -> Option<Vec<u8>> {
    let count = usize::from(width) * usize::from(height);
    let mut rgba = Vec::with_capacity(count * 4);
    match data {
        VideoData::Gray(luma) => {
            for value in luma.get(..count)? {
                rgba.extend_from_slice(&[*value, *value, *value, 255]);
            }
        }
        VideoData::Rgb(pixels) => {
            for pixel in pixels.get(..count * 3)?.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
    }
    Some(rgba)
}

#[must_use]
pub fn aspect_of(data: VideoData<'_>, width: u16, height: u16) -> f32 {
    match data {
        VideoData::Rgb(_) if height > 0 => f32::from(width) / f32::from(height),
        _ => DISPLAY_ASPECT,
    }
}

#[must_use]
pub fn caption(signal: Option<VideoSignal>) -> String {
    match signal {
        None => "waiting for sync".to_owned(),
        Some(signal) if signal.live => format!("{} \u{d7} {}", signal.width, signal.height),
        Some(signal) => format!("{} \u{d7} {} \u{b7} no sync", signal.width, signal.height),
    }
}

#[must_use]
pub fn picture_stats(inputs: &[(String, Option<VideoSignal>)]) -> Vec<Stat> {
    let several = inputs.len() > 1;
    inputs
        .iter()
        .map(|(name, signal)| {
            let label = if several { name.as_str() } else { "Picture" };
            let shown = Stat::new(label, caption(*signal), PICTURE_HINT);
            if signal.is_some_and(|signal| signal.live) {
                shown
            } else {
                shown.tone(Tone::Warn)
            }
        })
        .collect()
}

struct Shown {
    width: u16,
    height: u16,
    aspect: f32,
    rgba: Vec<u8>,
}

#[derive(Default)]
pub struct Picture {
    width: u32,
    height: u32,
    aspect: f32,
    rgba: Vec<u8>,
    stamp: u64,
}

impl Scene for Picture {
    fn stamp(&self) -> u64 {
        self.stamp
    }

    fn paint(&mut self, raster: &mut Raster) {
        if self.rgba.is_empty() {
            raster.fill([0, 0, 0, 255]);
            return;
        }
        raster.resize(self.width, self.height);
        raster.pixels.copy_from_slice(&self.rgba);
        raster.aspect = Some(self.aspect);
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("wp11-video", SHEET);
    kit_audio::install();
    let inputs = Memo::new(move |_| binding::sources_of(&store.graph.get(), &node, "video"));
    let signals: Signals = RwSignal::new(HashMap::new());
    let stats = move || {
        let held = signals.get();
        let named: Vec<(String, Option<VideoSignal>)> = inputs
            .get()
            .into_iter()
            .map(|source| {
                let signal = held.get(&source).copied();
                (kit_audio::node_label(store, &source, "video"), signal)
            })
            .collect();
        picture_stats(&named)
    };
    view! {
        column(class = "face") {
            if move || inputs.get().is_empty() {
                text(class = "hint") {"Wire a video channel's picture in"}
            }
            for source in move || inputs.get(), key = |source: &String| source.clone() {
                {screen(store, source, signals)}
            }
            if move || !inputs.get().is_empty() {
                row(class = "face__foot rec-foot") {
                    {kit_audio::stats(stats)}
                }
            }
        }
    }
}

fn screen(store: Store, source: String, signals: Signals) -> impl IntoView {
    let target = {
        let source = source.clone();
        Memo::new(move |_| {
            let set = store.device_set_of(&source)?;
            Some((set, store.channel_of(&source)?.id))
        })
    };
    move || {
        target
            .get()
            .map(|key| AnyView::new(watch(store, key, source.clone(), signals)))
    }
}

fn subscribe_with_grace(store: Store, key: Key, timers: Option<Timers>) {
    let command = ClientCommand::SubscribeVideo {
        device_set: key.0,
        channel: key.1,
    };
    let lingering = GRACE.with(|grace| grace.borrow_mut().remove(&key));
    if lingering.is_none() {
        store.acquire(&command);
    }
    on_cleanup_local(move || {
        let waiting = GRACE.with(|grace| grace.borrow().contains_key(&key));
        let Some(timers) = timers.filter(|_| !waiting) else {
            store.release(&command);
            return;
        };
        let handle = timers.set_timeout(RELEASE_GRACE, move || {
            let fired = GRACE.with(|grace| grace.borrow_mut().remove(&key));
            store.release(&command);
            drop(fired);
        });
        GRACE.with(|grace| grace.borrow_mut().insert(key, handle));
    });
}

struct Player {
    picture: Rc<RefCell<Picture>>,
    pacer: RefCell<Pacer<Shown>>,
    timer: RefCell<Option<TimeoutHandle>>,
    timers: Option<Timers>,
    geometry: RwSignal<Option<(u16, u16)>>,
    aspect: RwSignal<f32>,
}

impl Player {
    fn show(&self, shown: Shown) {
        if let Ok(mut picture) = self.picture.try_borrow_mut() {
            picture.width = u32::from(shown.width);
            picture.height = u32::from(shown.height);
            picture.aspect = shown.aspect;
            picture.rgba = shown.rgba;
            picture.stamp += 1;
        }
        if self.geometry.get_untracked() != Some((shown.width, shown.height)) {
            self.geometry.set(Some((shown.width, shown.height)));
        }
        if self.aspect.get_untracked() != shown.aspect {
            self.aspect.set(shown.aspect);
        }
    }

    fn present(self: &Rc<Self>) {
        let now = Instant::now();
        let due = self.pacer.borrow_mut().take_due(now);
        if let Some(shown) = due {
            self.show(shown);
        }
        let next = self.pacer.borrow().next_due();
        let handle = next.zip(self.timers.as_ref()).map(|(at, timers)| {
            let me = Rc::clone(self);
            timers.set_timeout(at.saturating_duration_since(now), move || me.present())
        });
        *self.timer.borrow_mut() = handle;
    }

    fn stop(&self) {
        self.timer.borrow_mut().take();
        self.pacer.borrow_mut().clear();
    }
}

fn decode(bytes: &[u8], key: Key) -> Option<Shown> {
    let Ok(decoded) = VideoFrame::decode(bytes) else {
        tracing::warn!(
            device_set = key.0,
            channel = key.1,
            "a video frame did not decode"
        );
        return None;
    };
    let Some(rgba) = rgba_of(decoded.data, decoded.width, decoded.height) else {
        tracing::warn!(
            device_set = key.0,
            channel = key.1,
            "a video frame was cut short"
        );
        return None;
    };
    Some(Shown {
        width: decoded.width,
        height: decoded.height,
        aspect: aspect_of(decoded.data, decoded.width, decoded.height),
        rgba,
    })
}

fn audio_delay(audio: Option<Audio>, route: &AudioRoute) -> Duration {
    let delay = audio.map_or(0.0, |audio| audio.latency_ms(route));
    Duration::from_secs_f64(clamp_delay_ms(delay) / 1_000.0)
}

fn watch(store: Store, key: Key, source: String, signals: Signals) -> impl IntoView {
    let timers = Timers::current();
    subscribe_with_grace(store, key, timers.clone());
    let player = Rc::new(Player {
        picture: Rc::new(RefCell::new(Picture::default())),
        pacer: RefCell::new(Pacer::default()),
        timer: RefCell::new(None),
        timers,
        geometry: RwSignal::new(None),
        aspect: RwSignal::new(DISPLAY_ASPECT),
    });
    let (geometry, aspect) = (player.geometry, player.aspect);
    let live = RwSignal::new(false);
    let seen = Rc::new(RefCell::new(None::<Instant>));
    let audio = use_audio();
    let route = AudioRoute {
        device_set: key.0,
        channel: key.1,
        fx: Vec::new(),
    };
    let heard = seen.clone();
    let playing = Rc::clone(&player);
    store.on_frame(move |frame| {
        let ours = store.source_of(frame.stream_id)
            == Some(Source::Video {
                device_set: key.0,
                channel: key.1,
            });
        if !matches!(frame.kind, FrameKind::VideoGray | FrameKind::VideoRgb) || !ours {
            return;
        }
        let Some(shown) = decode(&frame.bytes, key) else {
            return;
        };
        let delay = audio_delay(audio, &route);
        let size = shown.rgba.len();
        let dropped = playing
            .pacer
            .borrow_mut()
            .push(Instant::now() + delay, size, shown);
        if dropped > 0 {
            tracing::warn!(
                device_set = key.0,
                channel = key.1,
                dropped,
                "video frames dropped while waiting for audio"
            );
        }
        playing.present();
        *heard.borrow_mut() = Some(Instant::now());
        if !live.get_untracked() {
            live.set(true);
        }
    });
    let check = set_interval(STALE_CHECK, move || {
        let fresh = seen.borrow().is_some_and(|at| at.elapsed() < STALE);
        if live.get_untracked() != fresh {
            live.set(fresh);
        }
    });
    let reporting = zgui::reactive::RenderEffect::new({
        let source = source.clone();
        move |_| {
            let signal = geometry.get().map(|(width, height)| VideoSignal {
                width,
                height,
                live: live.get(),
            });
            let source = source.clone();
            signals.try_update(move |held| match signal {
                Some(signal) => held.insert(source, signal),
                None => held.remove(&source),
            });
        }
    });
    let stopping = Rc::clone(&player);
    on_cleanup_local(move || {
        drop(check);
        drop(reporting);
        stopping.stop();
        signals.try_update(|held| held.remove(&source));
    });
    let picture = Rc::clone(&player.picture);
    view! {
        box(class = "vid", style:aspect-ratio = move || Some(format!("{}", aspect.get()))) {
            {raster_view("vid__screen", picture)}
        }
    }
}

const SHEET: &str = css!(
    r#"
.vid { width: 100%; border-radius: 2px; overflow: hidden; background-color: #000; }
.vid__screen { width: 100%; height: 100%; }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grey_picture_is_spread_over_all_three_colours() {
        let rgba = rgba_of(VideoData::Gray(&[10, 20]), 2, 1).expect("a picture");
        assert_eq!(rgba, vec![10, 10, 10, 255, 20, 20, 20, 255]);
    }

    #[test]
    fn a_colour_picture_keeps_its_colours() {
        let rgba = rgba_of(VideoData::Rgb(&[1, 2, 3]), 1, 1).expect("a picture");
        assert_eq!(rgba, vec![1, 2, 3, 255]);
    }

    #[test]
    fn a_picture_shorter_than_its_size_is_refused() {
        assert_eq!(rgba_of(VideoData::Gray(&[1, 2, 3]), 2, 2), None);
        assert_eq!(rgba_of(VideoData::Rgb(&[1, 2, 3]), 2, 1), None);
    }

    #[test]
    fn analogue_television_is_shown_at_four_by_three() {
        assert!((aspect_of(VideoData::Gray(&[]), 400, 300) - DISPLAY_ASPECT).abs() < 1e-6);
        assert!((aspect_of(VideoData::Gray(&[]), 100, 100) - DISPLAY_ASPECT).abs() < 1e-6);
        assert!((aspect_of(VideoData::Rgb(&[]), 320, 160) - 2.0).abs() < 1e-6);
    }

    fn signal(live: bool) -> Option<VideoSignal> {
        Some(VideoSignal {
            width: 320,
            height: 240,
            live,
        })
    }

    #[test]
    fn the_caption_says_whether_the_picture_is_still_coming() {
        assert_eq!(caption(None), "waiting for sync");
        assert_eq!(caption(signal(true)), "320 \u{d7} 240");
        assert_eq!(caption(signal(false)), "320 \u{d7} 240 \u{b7} no sync");
    }

    #[test]
    fn several_inputs_are_named_and_a_dead_picture_warns() {
        let one = picture_stats(&[(String::from("ATV"), signal(true))]);
        assert_eq!(
            (one[0].label.as_str(), one[0].tone),
            ("Picture", Tone::Plain)
        );
        let two = picture_stats(&[
            (String::from("ATV"), signal(true)),
            (String::from("SSTV"), None),
        ]);
        assert_eq!(two[0].label, "ATV");
        assert_eq!((two[1].label.as_str(), two[1].tone), ("SSTV", Tone::Warn));
    }

    #[test]
    fn a_picture_paints_at_its_own_size_and_aspect() {
        let mut picture = Picture {
            width: 2,
            height: 1,
            aspect: 2.0,
            rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
            stamp: 1,
        };
        let mut raster = Raster::sized(10, 10);
        picture.paint(&mut raster);
        assert_eq!((raster.width, raster.height), (2, 1));
        assert_eq!(raster.aspect, Some(2.0));
        assert_eq!(raster.at(1, 0), Some([4, 5, 6, 255]));
    }
}
