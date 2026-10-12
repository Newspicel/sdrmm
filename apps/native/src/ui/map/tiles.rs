use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

use zgui::reactive::ArcRwSignal;
use zgui::reactive::RenderEffect;
use zgui::{
    elements::{kurbo, kurbo::Shape as _},
    prelude::*,
};

use super::{
    geo::{TileId, View},
    paint::{EXTENT, LabelClass, Painted},
    source::{Basemap, Cache, Kind, Net, OFFLINE_PATH, Slot},
    style::{BLANK_GROUND, BasemapChoice, Palette, Preset, halo, hex},
};
use crate::store::Store;

const CACHED_TILES: usize = 384;
const PARENTS_TRIED: u8 = 5;
const PLACES_SHOWN: usize = 28;
const PLACE_GAP_X: f64 = 90.0;
const PLACE_GAP_Y: f64 = 18.0;

pub struct Shared {
    basemap: ArcRwSignal<Option<Basemap>>,
    preset: ArcRwSignal<Preset>,
    arrivals: ArcRwSignal<u64>,
    failed: ArcRwSignal<u32>,
    cache: RefCell<Cache>,
    net: Option<Net>,
    wanted: RefCell<Option<BasemapChoice>>,
    vector: RefCell<Option<Basemap>>,
    generation: Cell<u64>,
}

thread_local! {
    static SHARED: Rc<Shared> = Rc::new(Shared::new());
}

impl Shared {
    fn new() -> Self {
        let net = match Net::new() {
            Ok(net) => Some(net),
            Err(error) => {
                tracing::warn!(%error, "the map cannot fetch tiles");
                None
            }
        };
        Self {
            basemap: ArcRwSignal::new(None),
            preset: ArcRwSignal::new(Preset::default()),
            arrivals: ArcRwSignal::new(0),
            failed: ArcRwSignal::new(0),
            cache: RefCell::new(Cache::new(CACHED_TILES)),
            net,
            wanted: RefCell::new(None),
            vector: RefCell::new(None),
            generation: Cell::new(0),
        }
    }

    pub fn get() -> Rc<Self> {
        SHARED.with(Rc::clone)
    }

    pub fn ensure(self: &Rc<Self>, store: Store, choice: &BasemapChoice) {
        if self.wanted.borrow().as_ref() == Some(choice) {
            return;
        }
        self.wanted.replace(Some(choice.clone()));
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.failed.set(0);
        self.preset.set(choice.preset);
        let Some(net) = self.net.clone() else {
            self.basemap.set(Some(Basemap::Blank));
            store.say("the map cannot reach the network");
            return;
        };
        if choice.preset != Preset::Custom
            && let Some(found) = self.vector.borrow().clone()
        {
            self.basemap.set(Some(found));
            return;
        }
        self.basemap.set(None);
        let shared = self.clone();
        let custom = (choice.preset == Preset::Custom).then(|| choice.custom.clone());
        let url = store.api().url(OFFLINE_PATH);
        zgui::task::spawn_detached(async move {
            let found = zgui::task::background(async move {
                match custom {
                    Some(custom) => net.custom(&custom).await.map_err(|error| error.to_string()),
                    None => Ok(net.discover(url).await),
                }
            })
            .await;
            shared.settle_basemap(store, generation, found);
        });
    }

    fn settle_basemap(&self, store: Store, generation: u64, found: Result<Basemap, String>) {
        if self.generation.get() != generation {
            return;
        }
        let found = found.unwrap_or_else(|error| {
            store.say(format!("Custom map: {error}"));
            Basemap::Blank
        });
        if found.is_vector() && self.preset.get_untracked() != Preset::Custom {
            self.vector.replace(Some(found.clone()));
        }
        self.basemap.set(Some(found));
    }

    pub fn kind(&self) -> Kind {
        self.basemap
            .get()
            .as_ref()
            .map_or(Kind::Pending, Basemap::kind)
    }

    pub fn ground(&self) -> u32 {
        match self.kind() {
            Kind::Online | Kind::Offline => self.preset.get().palette().land,
            Kind::Pending | Kind::Blank => BLANK_GROUND,
        }
    }

    pub fn failures(&self) -> u32 {
        self.failed.get()
    }

    pub fn credit(&self) -> &'static str {
        self.basemap.get().as_ref().map_or("", Basemap::attribution)
    }

    fn key_of(&self, basemap: &Basemap) -> String {
        format!("{}|{}", self.preset.get_untracked().key(), basemap.key())
    }

    fn request(self: &Rc<Self>, basemap: &Basemap, id: TileId) {
        let Some(net) = self.net.clone() else {
            return;
        };
        let source = self.key_of(basemap);
        if !self.cache.borrow_mut().claim(&source, id) {
            return;
        }
        let shared = self.clone();
        let basemap = basemap.clone();
        let palette = self.preset.get_untracked().palette();
        zgui::task::spawn_detached(async move {
            let fetched =
                zgui::task::background(async move { net.tile(&basemap, id, &palette).await }).await;
            let slot = match fetched {
                Ok(painted) => Slot::Ready(Arc::new(painted)),
                Err(error) => {
                    tracing::debug!(%error, ?id, "a map tile failed");
                    shared.failed.update(|count| *count += 1);
                    Slot::Failed
                }
            };
            shared.cache.borrow_mut().settle(&source, id, slot);
            shared.arrivals.update(|count| *count += 1);
        });
    }

    fn ready(&self, source: &str, id: TileId) -> Option<Arc<Painted>> {
        self.cache.borrow().ready(source, id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Drawn {
    key: String,
    source: String,
    id: TileId,
    column: i64,
}

fn place(view: &View, id: TileId, column: i64) -> (f64, f64, f64) {
    let world = view.world();
    let size = world / f64::from(1u32 << id.z);
    let left = view.width / 2.0 - view.x * world + column as f64 * size;
    let top = view.height / 2.0 - view.y * world + f64::from(id.y) * size;
    (left, top, size)
}

fn clipped(view: &View, left: f64, top: f64, size: f64) -> Option<[f64; 4]> {
    let (x0, y0) = (left.max(0.0), top.max(0.0));
    let (x1, y1) = ((left + size).min(view.width), (top + size).min(view.height));
    (x1 > x0 && y1 > y0 && size > 0.0).then_some([x0, y0, x1 - x0, y1 - y0])
}

fn visible(view: &View, id: TileId, column: i64) -> (String, Option<[f64; 4]>) {
    let (left, top, size) = place(view, id, column);
    let style =
        format!("left: {left:.2}px; top: {top:.2}px; width: {size:.2}px; height: {size:.2}px");
    let window = clipped(view, left, top, size).map(|[x, y, width, height]| {
        let unit = EXTENT / size;
        [
            (x - left) * unit,
            (y - top) * unit,
            width * unit,
            height * unit,
        ]
    });
    (style, window)
}

fn drawn(shared: &Shared, basemap: &Basemap, view: &View) -> Vec<Drawn> {
    let source = shared.key_of(basemap);
    let source = source.as_str();
    let mut children = Vec::new();
    let mut parents = Vec::new();
    let mut seen = HashSet::new();
    for spot in view.tiles_at(basemap.max_zoom(), basemap.zoom_bump()) {
        if shared.ready(source, spot.id).is_some() {
            children.push(Drawn {
                key: format!("{source}#{}/{}/{}", spot.id.z, spot.column, spot.id.y),
                source: source.to_owned(),
                id: spot.id,
                column: spot.column,
            });
            continue;
        }
        let (mut id, mut column) = (spot.id, spot.column);
        for _ in 0..PARENTS_TRIED {
            let Some(parent) = id.parent() else {
                break;
            };
            id = parent;
            column = column.div_euclid(2);
            if shared.ready(source, id).is_some() {
                let key = format!("{source}#{}/{}/{}", id.z, column, id.y);
                if seen.insert(key.clone()) {
                    parents.push(Drawn {
                        key,
                        source: source.to_owned(),
                        id,
                        column,
                    });
                }
                break;
            }
        }
    }
    parents.sort_by_key(|tile| tile.id.z);
    parents.extend(children);
    parents
}

fn current<T>(
    shared: &Shared,
    view: RwSignal<View>,
    read: fn(&Shared, &Basemap, &View) -> Vec<T>,
) -> Vec<T> {
    shared.arrivals.with(|_| ());
    shared.preset.with(|_| ());
    shared
        .basemap
        .get()
        .map(|basemap| read(shared, &basemap, &view.get()))
        .unwrap_or_default()
}

pub fn layer(shared: Rc<Shared>, view: RwSignal<View>) -> impl IntoView {
    let fetching = {
        let shared = shared.clone();
        RenderEffect::new(move |_| {
            let Some(basemap) = shared.basemap.get() else {
                return;
            };
            shared.preset.with(|_| ());
            let current = view.get();
            for spot in current.tiles_at(basemap.max_zoom(), basemap.zoom_bump()) {
                shared.request(&basemap, spot.id);
            }
        })
    };
    on_cleanup_local(move || drop(fetching));

    let tiles = shared.clone();
    let places = shared.clone();
    let palette = shared.preset.clone();

    let row = move |tile: Drawn| {
        let painted = shared.ready(&tile.source, tile.id);
        let (id, column) = (tile.id, tile.column);
        if let Some(picture) = painted.as_ref().and_then(|painted| painted.picture.clone()) {
            let url = picture.url();
            on_cleanup_local(move || drop(picture));
            return AnyView::new(
                zgui::elements::image()
                    .class("map__tile")
                    .src(Some(url))
                    .alt(Some(String::new()))
                    .style_text(move || Some(visible(&view.get(), id, column).0)),
            );
        }
        let canvas = zgui::elements::canvas()
            .class("map__tile")
            .view_box(0.0, 0.0, EXTENT as f32, EXTENT as f32)
            .style_text(move || Some(visible(&view.get(), id, column).0))
            .draw(move |cx| {
                let Some(painted) = &painted else {
                    return;
                };
                let Some([x, y, width, height]) = visible(&view.get(), id, column).1 else {
                    return;
                };
                let window = Arc::new(kurbo::Rect::new(x, y, x + width, y + height).to_path(0.1));
                for shape in &painted.shapes {
                    let mut shape = shape.clone();
                    for clip in &mut shape.clips {
                        clip.path = window.clone();
                    }
                    cx.scene.push(shape);
                }
            })
            .into_view();
        AnyView::new(canvas)
    };

    view! {
        box(class = "map__layer") {
            for tile in move || current(&tiles, view, drawn), key = |tile: &Drawn| tile.key.clone() {
                {row(tile)}
            }
        }
        box(class = "map__layer") {
            for spot in move || current(&places, view, places_of), key = |spot: &Place| spot.key.clone() {
                {place_label(spot, view, palette.get_untracked().palette())}
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Place {
    key: String,
    text: String,
    class: LabelClass,
    x: f64,
    y: f64,
}

fn places_of(shared: &Shared, basemap: &Basemap, view: &View) -> Vec<Place> {
    let source = shared.key_of(basemap);
    let mut candidates: Vec<Place> = Vec::new();
    let mut names = HashSet::new();
    let mut ranked: Vec<(i64, Place)> = Vec::new();
    for spot in view.tiles_at(basemap.max_zoom(), basemap.zoom_bump()) {
        let Some(painted) = shared.ready(&source, spot.id) else {
            continue;
        };
        let shift = (spot.column - i64::from(spot.id.x)) as f64 / f64::from(1u32 << spot.id.z);
        for label in &painted.labels {
            ranked.push((
                label.score,
                Place {
                    key: format!("{source}:{}:{:.5}:{:.5}", label.text, label.x, label.y),
                    text: label.text.clone(),
                    class: label.class,
                    x: label.x + shift,
                    y: label.y,
                },
            ));
        }
    }
    ranked.sort_by_key(|(score, _)| *score);
    for (_, place) in ranked {
        let (sx, sy) = view.screen_of_unit(place.x, place.y);
        let inside = (0.0..view.width).contains(&sx) && (0.0..view.height).contains(&sy);
        let crowded = candidates.iter().any(|kept| {
            let (kx, ky) = view.screen_of_unit(kept.x, kept.y);
            (kx - sx).abs() < PLACE_GAP_X && (ky - sy).abs() < PLACE_GAP_Y
        });
        if inside && !crowded && names.insert(place.text.clone()) {
            candidates.push(place);
            if candidates.len() >= PLACES_SHOWN {
                break;
            }
        }
    }
    candidates
}

fn place_label(spot: Place, view: RwSignal<View>, palette: Palette) -> impl IntoView {
    let (x, y) = (spot.x, spot.y);
    let at = move || view.with(|view| view.screen_of_unit(x, y));
    let colour = match spot.class {
        LabelClass::Country | LabelClass::State => palette.place_faint,
        LabelClass::City | LabelClass::Town | LabelClass::Village => palette.place,
    };
    view! {
        text(
            class = "map__place",
            attr:data-class = spot.class.css(),
            style:color = Some(hex(colour)),
            style:text-shadow = Some(halo(palette.land)),
            style:left = move || Some(format!("{:.1}px", at().0)),
            style:top = move || Some(format!("{:.1}px", at().1))
        ) {{spot.text}}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_is_cut_to_the_part_the_map_shows() {
        let view = View {
            width: 400.0,
            height: 300.0,
            ..View::default()
        };
        assert_eq!(
            clipped(&view, -100.0, 50.0, 512.0),
            Some([0.0, 50.0, 400.0, 250.0])
        );
        assert_eq!(clipped(&view, 500.0, 0.0, 512.0), None);
        let (style, window) = visible(&view, TileId { z: 0, x: 0, y: 0 }, 0);
        assert!(style.starts_with("left: -"));
        let [x, y, width, height] = window.expect("a visible part");
        assert!(x > 0.0 && y > 0.0 && width > 0.0 && height > 0.0);
        assert!(x + width <= EXTENT && y + height <= EXTENT);
    }
}
