use sdrmm_wire::{
    array::ArrayGeometry,
    limits::DF_LIMITS as LIMITS,
    patch::{NodeBody, PatchGraph},
    processor::df::{DfAlgorithm, DfParams, SourceRule, UlaSide},
};
use zgui::{prelude::*, vocab::SharedString};

use super::{
    geometry::{allows_structured, is_collinear, is_level_line},
    options::{
        AUTO_SOURCES, algorithm_options, algorithm_title, elevation_block, elevation_locked,
        from_khz, peak_options, read_squelch, rule_options, rule_title, side_options, side_title,
        smoothing_locked, smoothing_title, source_options, sources_title, squelch_text, station_id,
        to_khz,
    },
};
use crate::{
    store::Store,
    ui::{
        faces::processor,
        kit_raster::{Bounds, number},
        kit_sources::collapsible,
        params::entry,
        widgets::{check, pick, row_field},
    },
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrayShape {
    pub geometry: Option<ArrayGeometry>,
    pub lanes: u32,
}

impl ArrayShape {
    #[must_use]
    pub fn of(graph: &PatchGraph, array: Option<&str>) -> Self {
        let Some(array) = array else {
            return Self::default();
        };
        let geometry = graph.node(array).and_then(|found| match &found.body {
            NodeBody::Array(node) => Some(node.geometry.clone()),
            _ => None,
        });
        Self {
            geometry,
            lanes: graph.array_wired_lanes(array),
        }
    }

    #[must_use]
    pub fn structured_ok(&self) -> bool {
        self.geometry
            .as_ref()
            .is_none_or(|geometry| allows_structured(geometry, self.lanes))
    }

    #[must_use]
    pub fn collinear(&self) -> bool {
        self.geometry
            .as_ref()
            .is_some_and(|geometry| is_collinear(geometry, self.lanes))
    }

    #[must_use]
    pub fn level_line(&self) -> bool {
        self.geometry
            .as_ref()
            .is_some_and(|geometry| is_level_line(geometry, self.lanes))
    }
}

pub fn df_of(body: &NodeBody) -> Option<&DfParams> {
    match body {
        NodeBody::Df(node) => Some(&node.settings),
        _ => None,
    }
}

fn df_of_mut(body: &mut NodeBody) -> Option<&mut DfParams> {
    match body {
        NodeBody::Df(node) => Some(&mut node.settings),
        _ => None,
    }
}

#[derive(Clone)]
pub struct Finder {
    pub store: Store,
    pub node: String,
    pub settings: Memo<Option<DfParams>>,
    pub shape: Memo<ArrayShape>,
}

impl Finder {
    fn read<V>(&self, pick: fn(&DfParams) -> V) -> Signal<V>
    where
        V: Default + Clone + Send + Sync + 'static,
    {
        let settings = self.settings;
        Signal::derive(move || settings.with(|found| found.as_ref().map(pick).unwrap_or_default()))
    }

    fn write(&self, change: impl FnOnce(&mut DfParams)) {
        processor::edit(self.store, &self.node, df_of_mut, change);
    }

    fn setter<V: 'static>(&self, apply: fn(&mut DfParams, V)) -> impl Fn(V) + Clone + 'static {
        let finder = self.clone();
        move |value: V| finder.write(|params| apply(params, value))
    }
}

fn titled(title: impl Fn() -> String + 'static, body: impl IntoView + 'static) -> impl IntoView {
    view! { box(a11y:description = move || SharedString::from(title())) {{body}} }
}

fn method(finder: &Finder) -> impl IntoView {
    let (settings, shape) = (finder.settings, finder.shape);
    let current = Memo::new(move |_| settings.with(|found| found.as_ref().map(|p| p.algorithm)));
    let structured = Memo::new(move |_| shape.with(ArrayShape::structured_ok));
    let options =
        Memo::new(move |_| algorithm_options(structured.get(), current.get().unwrap_or_default()));
    let on_pick = finder.setter(|params, algorithm: DfAlgorithm| params.algorithm = algorithm);
    let title =
        move || algorithm_title(current.get().unwrap_or_default(), structured.get()).to_owned();
    let body = move || {
        let on_pick = on_pick.clone();
        row_field("Method", pick(options.get(), current.into(), on_pick))
    };
    titled(title, body)
}

fn sources(finder: &Finder) -> impl IntoView {
    let (settings, shape) = (finder.settings, finder.shape);
    let stored = Memo::new(move |_| settings.with(|found| found.as_ref().and_then(|p| p.sources)));
    let lanes = Memo::new(move |_| shape.with(|shape| shape.lanes));
    let options = Memo::new(move |_| source_options(lanes.get(), stored.get()));
    let chosen = Signal::derive(move || Some(stored.get().unwrap_or(AUTO_SOURCES)));
    let on_pick = finder.setter(|params, count: u32| {
        params.sources = (count != AUTO_SOURCES).then_some(count);
    });
    let title = move || sources_title(lanes.get(), stored.get()).to_owned();
    let body = move || {
        let on_pick = on_pick.clone();
        row_field("Sources", pick(options.get(), chosen, on_pick))
    };
    titled(title, body)
}

fn rule(finder: &Finder) -> impl IntoView {
    let chosen = finder.read(|params| Some(params.source_rule));
    let on_pick = finder.setter(|params, rule: SourceRule| params.source_rule = rule);
    titled(
        move || chosen.get().map(rule_title).unwrap_or_default().to_owned(),
        row_field("Rule", pick(rule_options(), chosen, on_pick)),
    )
}

fn peaks(finder: &Finder) -> impl IntoView {
    let chosen = finder.read(|params| Some(params.max_peaks));
    let on_pick = finder.setter(|params, count: u8| params.max_peaks = count);
    titled(
        || "Most bearings per report".to_owned(),
        row_field("Peaks", pick(peak_options(), chosen, on_pick)),
    )
}

fn band(finder: &Finder) -> impl IntoView {
    let band = LIMITS.band;
    let offset = finder.read(|params| to_khz(params.offset_hz));
    let width = finder.read(|params| to_khz(params.bandwidth_hz));
    let set_offset = finder.setter(|params, khz: f64| params.offset_hz = from_khz(khz));
    let set_width = finder.setter(|params, khz: f64| params.bandwidth_hz = from_khz(khz));
    view! {
        {titled(|| "Signal offset from the array centre".to_owned(), row_field("Offset kHz", number(
            "Offset",
            offset,
            Bounds::new(to_khz(band.offset_hz.min), to_khz(band.offset_hz.max)),
            set_offset,
        )))}
        {titled(|| "Band the finder listens to".to_owned(), row_field("Width kHz", number(
            "Width",
            width,
            Bounds::new(to_khz(band.bandwidth_hz.min), to_khz(band.bandwidth_hz.max)),
            set_width,
        )))}
    }
}

fn squelch(finder: &Finder) -> impl IntoView {
    let shown = finder.read(|params| squelch_text(params.squelch_db));
    let finder = finder.clone();
    let commit = move |text: String| {
        let db = read_squelch(&text)?;
        finder.write(|params| params.squelch_db = db);
        Ok(())
    };
    titled(
        || "Minimum peak above the floor, 0 off".to_owned(),
        row_field(
            "Squelch dB",
            entry(shown, "Squelch".to_owned(), false, commit),
        ),
    )
}

fn report(finder: &Finder) -> impl IntoView {
    let value = finder.read(|params| f64::from(params.report_ms));
    let set = finder.setter(|params, ms: f64| params.report_ms = ms as u32);
    row_field(
        "Report ms",
        number(
            "Report",
            value,
            Bounds::whole(
                f64::from(LIMITS.report_ms.min),
                f64::from(LIMITS.report_ms.max),
            ),
            set,
        ),
    )
}

fn elevation(finder: &Finder) -> impl IntoView {
    let (settings, shape) = (finder.settings, finder.shape);
    let on = finder.read(|params| params.elevation);
    let block = Memo::new(move |_| {
        let collinear = shape.with(ArrayShape::collinear);
        settings.with(|found| {
            found
                .as_ref()
                .and_then(|p| elevation_block(collinear, p.algorithm, p.smoothing))
        })
    });
    let locked = Signal::derive(move || elevation_locked(block.get(), on.get()));
    let finder = finder.clone();
    let toggle = move |next: bool| {
        if !locked.get_untracked() {
            finder.write(|params| params.elevation = next);
        }
    };
    let title = move || block.get().unwrap_or("Estimate elevation too").to_owned();
    view! {
        box(class:df__off = move || locked.get(), a11y:description = move || SharedString::from(title())) {
            {row_field("Elevation", check(on, toggle))}
        }
    }
}

fn side(finder: &Finder) -> impl IntoView {
    let shape = finder.shape;
    let line = Memo::new(move |_| shape.with(ArrayShape::level_line));
    let chosen = finder.read(|params| Some(params.ula_side));
    let on_pick = finder.setter(|params, side: UlaSide| params.ula_side = side);
    move || {
        let on_pick = on_pick.clone();
        line.get().then(|| {
            AnyView::new(titled(
                move || chosen.get().map(side_title).unwrap_or_default().to_owned(),
                row_field("Side", pick(side_options(), chosen, on_pick)),
            ))
        })
    }
}

fn station(finder: &Finder) -> impl IntoView {
    let shown = finder.read(|params| params.station_id.clone().unwrap_or_default());
    let finder = finder.clone();
    let commit = move |text: String| {
        let id = station_id(&text)?;
        finder.write(|params| params.station_id = id);
        Ok(())
    };
    titled(
        || "Name on the bearings this finder sends".to_owned(),
        row_field("Station", entry(shown, "unnamed".to_owned(), true, commit)),
    )
}

pub fn main(finder: &Finder) -> impl IntoView {
    view! {
        column(class = "params") {
            {method(finder)}
            {band(finder)}
            {sources(finder)}
            {rule(finder)}
            {peaks(finder)}
            {squelch(finder)}
            {report(finder)}
            {elevation(finder)}
            {side(finder)}
            {station(finder)}
        }
    }
}

fn smoothing(finder: &Finder) -> impl IntoView {
    let shape = finder.shape;
    let structured = Memo::new(move |_| shape.with(ArrayShape::structured_ok));
    let value = finder.read(|params| params.smoothing);
    let locked = Memo::new(move |_| smoothing_locked(structured.get(), value.get()));
    let set = finder.setter(|params, count: f64| params.smoothing = count as u8);
    let body = move || {
        if locked.get() {
            return AnyView::new(view! { text(class = "kit-note") {"off"} });
        }
        AnyView::new(number(
            "Smooth",
            Signal::derive(move || f64::from(value.get())),
            Bounds::whole(
                f64::from(LIMITS.smoothing.min),
                f64::from(LIMITS.smoothing.max),
            ),
            set.clone(),
        ))
    };
    titled(
        move || smoothing_title(structured.get()).to_owned(),
        row_field("Smooth", body),
    )
}

fn more_fields(finder: &Finder) -> impl IntoView {
    let fb = finder.read(|params| params.forward_backward);
    let set_fb = finder.setter(|params, on: bool| params.forward_backward = on);
    let float = |bounds: sdrmm_wire::limits::Bounds<f32>| {
        Bounds::new(f64::from(bounds.min), f64::from(bounds.max))
    };
    view! {
        column(class = "params") {
            {titled(|| "Forward-backward averaging, needs a symmetric array".to_owned(),
                row_field("FB", check(fb, set_fb)))}
            {smoothing(finder)}
            {titled(|| "Diagonal loading, steadies Capon and MUSIC".to_owned(), row_field("Loading", number(
                "Loading",
                finder.read(|params| f64::from(params.loading)),
                float(LIMITS.loading),
                finder.setter(|params, value: f64| params.loading = value as f32),
            )))}
            {titled(|| "Scan grid".to_owned(), row_field("Step \u{b0}", number(
                "Step",
                finder.read(|params| params.azimuth_step_deg),
                Bounds::new(LIMITS.azimuth_step_deg.min, LIMITS.azimuth_step_deg.max),
                finder.setter(|params, value: f64| params.azimuth_step_deg = value),
            )))}
            {titled(|| "Skip blocks while the array turns faster".to_owned(), row_field("Yaw gate \u{b0}/s", number(
                "Yaw gate",
                finder.read(|params| f64::from(params.yaw_gate_dps)),
                float(LIMITS.yaw_gate_dps),
                finder.setter(|params, value: f64| params.yaw_gate_dps = value as f32),
            )))}
            {titled(|| "Share of the last report kept".to_owned(), row_field("Carry over", number(
                "Carry over",
                finder.read(|params| f64::from(params.carry_over)),
                float(LIMITS.carry_over),
                finder.setter(|params, value: f64| params.carry_over = value as f32),
            )))}
        }
    }
}

pub fn more(finder: &Finder) -> impl IntoView {
    let finder = finder.clone();
    collapsible(
        || "More".to_owned(),
        "kit-fold__link",
        move || AnyView::new(more_fields(&finder)),
    )
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::array::{ArrayElement, Winding};

    use super::*;

    #[test]
    fn an_unknown_array_never_refuses_a_method() {
        let unknown = ArrayShape::default();
        assert!(unknown.structured_ok());
        assert!(!unknown.collinear());
        assert!(!unknown.level_line());
    }

    #[test]
    fn a_wired_line_offers_the_side_and_refuses_elevation() {
        let line = ArrayShape {
            geometry: Some(ArrayGeometry::Ula {
                spacing_m: 0.4,
                axis_deg: 90.0,
            }),
            lanes: 4,
        };
        assert!(line.level_line());
        assert!(line.collinear());
        let circle = ArrayShape {
            geometry: Some(ArrayGeometry::Uca {
                radius_m: 0.3,
                first_deg: 0.0,
                winding: Winding::Clockwise,
            }),
            lanes: 4,
        };
        assert!(!circle.collinear());
        let scattered = ArrayShape {
            geometry: Some(ArrayGeometry::Explicit {
                positions: vec![
                    ArrayElement::default(),
                    ArrayElement {
                        x_m: 0.4,
                        y_m: 0.1,
                        z_m: 0.0,
                    },
                    ArrayElement {
                        x_m: 0.1,
                        y_m: 0.5,
                        z_m: 0.0,
                    },
                ],
            }),
            lanes: 3,
        };
        assert!(!scattered.structured_ok());
    }

    #[test]
    fn the_settings_are_read_only_from_a_finder_body() {
        assert!(df_of(&NodeBody::Scope).is_none());
        let mut body = NodeBody::Df(sdrmm_wire::DfNode::default());
        assert_eq!(df_of(&body), Some(&DfParams::default()));
        if let Some(params) = df_of_mut(&mut body) {
            params.max_peaks = 3;
        }
        assert_eq!(df_of(&body).map(|params| params.max_peaks), Some(3));
    }
}
