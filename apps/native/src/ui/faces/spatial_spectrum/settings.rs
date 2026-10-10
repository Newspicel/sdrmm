use sdrmm_wire::{
    limits::SPATIAL_LIMITS as LIMITS,
    patch::NodeBody,
    processor::{
        DEFAULT_BAND_HZ,
        spatial::{SpatialMethod, SpatialSpectrumParams},
    },
};
use zgui::{prelude::*, vocab::SharedString};

use super::logic::{
    bin_options, colour_options, column_options, method_options, method_title, step_options,
    with_bins,
};
use crate::{
    store::Store,
    ui::{
        faces::{
            df::options::{from_khz, to_khz},
            processor,
            scope::colormap::Colormap,
        },
        kit_raster::{Bounds, number},
        widgets::{check, pick, row_field},
    },
};

pub fn spatial_of(body: &NodeBody) -> Option<&SpatialSpectrumParams> {
    match body {
        NodeBody::SpatialSpectrum(node) => Some(&node.settings),
        _ => None,
    }
}

fn spatial_of_mut(body: &mut NodeBody) -> Option<&mut SpatialSpectrumParams> {
    match body {
        NodeBody::SpatialSpectrum(node) => Some(&mut node.settings),
        _ => None,
    }
}

#[derive(Clone)]
pub struct Spatial {
    pub store: Store,
    pub node: String,
    pub settings: Memo<Option<SpatialSpectrumParams>>,
}

impl Spatial {
    pub fn read<V>(&self, pick: fn(&SpatialSpectrumParams) -> V) -> Signal<V>
    where
        V: Default + Clone + Send + Sync + 'static,
    {
        let settings = self.settings;
        Signal::derive(move || settings.with(|found| found.as_ref().map(pick).unwrap_or_default()))
    }

    fn setter<V: 'static>(
        &self,
        apply: fn(&mut SpatialSpectrumParams, V),
    ) -> impl Fn(V) + Clone + 'static {
        let (store, node) = (self.store, self.node.clone());
        move |value: V| {
            processor::edit(store, &node, spatial_of_mut, |params| apply(params, value));
        }
    }
}

fn titled(title: &'static str, body: impl IntoView + 'static) -> impl IntoView {
    view! { box(a11y:description = title) {{body}} }
}

fn whole(bounds: sdrmm_wire::limits::Bounds<u32>) -> Bounds {
    Bounds::whole(f64::from(bounds.min), f64::from(bounds.max))
}

fn method(spatial: &Spatial) -> impl IntoView {
    let chosen = spatial.read(|params| Some(params.method));
    let on_pick = spatial.setter(|params, method: SpatialMethod| params.method = method);
    view! {
        box(a11y:description = move || SharedString::from(chosen.get().map(method_title).unwrap_or("Method"))) {
            {row_field("Method", pick(method_options(), chosen, on_pick))}
        }
    }
}

fn grid(spatial: &Spatial) -> impl IntoView {
    let bins = spatial.read(|params| Some(params.bins));
    let columns = spatial.read(|params| Some(params.columns));
    let set_bins = spatial.setter(|params, bins: u32| *params = with_bins(params, bins));
    let set_columns = spatial.setter(|params, columns: u32| params.columns = columns);
    let options =
        Memo::new(move |_| column_options(bins.get().unwrap_or(0), columns.get().unwrap_or(0)));
    view! {
        {titled("Frequency bins", row_field("FFT", pick(bin_options(), bins, set_bins)))}
        {titled("Frequency columns shown", row_field("Columns", move || {
            pick(options.get(), columns, set_columns.clone())
        }))}
    }
}

fn timing(spatial: &Spatial) -> impl IntoView {
    view! {
        {titled("Average", row_field("Average ms", number(
            "Average",
            spatial.read(|params| f64::from(params.average_ms)),
            whole(LIMITS.average_ms),
            spatial.setter(|params, ms: f64| params.average_ms = ms as u32),
        )))}
        {titled("New picture this often", row_field("Rate ms", number(
            "Rate",
            spatial.read(|params| f64::from(params.report_ms)),
            whole(LIMITS.report_ms),
            spatial.setter(|params, ms: f64| params.report_ms = ms as u32),
        )))}
    }
}

fn step(spatial: &Spatial) -> impl IntoView {
    let chosen = spatial.read(|params| Some(params.azimuth_step_deg.round() as u32));
    let on_pick = spatial.setter(|params, deg: u32| params.azimuth_step_deg = f64::from(deg));
    titled(
        "Bearing resolution",
        row_field("Step", pick(step_options(), chosen, on_pick)),
    )
}

fn range(spatial: &Spatial) -> impl IntoView {
    titled(
        "Colour range below the peak",
        row_field(
            "Range dB",
            number(
                "Range",
                spatial.read(|params| f64::from(params.span_db)),
                Bounds::new(f64::from(LIMITS.span_db.min), f64::from(LIMITS.span_db.max)),
                spatial.setter(|params, db: f64| params.span_db = db as f32),
            ),
        ),
    )
}

fn band(spatial: &Spatial) -> impl IntoView {
    let limits = LIMITS.band;
    let full = spatial.read(|params| params.bandwidth_hz.is_none());
    let set_full = spatial.setter(|params, on: bool| {
        params.bandwidth_hz = (!on).then_some(DEFAULT_BAND_HZ);
    });
    let width = spatial.read(|params| params.bandwidth_hz.map(to_khz).unwrap_or_default());
    let set_width = spatial.setter(|params, khz: f64| params.bandwidth_hz = Some(from_khz(khz)));
    let width_field = move || {
        let set_width = set_width.clone();
        (!full.get()).then(|| {
            AnyView::new(titled(
                "Band around the offset",
                row_field(
                    "Width kHz",
                    number(
                        "Width",
                        width,
                        Bounds::new(
                            to_khz(limits.bandwidth_hz.min),
                            to_khz(limits.bandwidth_hz.max),
                        ),
                        set_width,
                    ),
                ),
            ))
        })
    };
    view! {
        {titled("From the array centre", row_field("Offset kHz", number(
            "Offset",
            spatial.read(|params| to_khz(params.offset_hz)),
            Bounds::new(to_khz(limits.offset_hz.min), to_khz(limits.offset_hz.max)),
            spatial.setter(|params, khz: f64| params.offset_hz = from_khz(khz)),
        )))}
        {titled("Every lane sample", row_field("Full band", check(full, set_full)))}
        {width_field}
    }
}

fn colours(colormap: RwSignal<Colormap>) -> impl IntoView {
    titled(
        "Colours",
        row_field(
            "Colours",
            pick(
                colour_options(),
                Signal::derive(move || Some(colormap.get())),
                move |chosen| colormap.set(chosen),
            ),
        ),
    )
}

pub fn settings(spatial: &Spatial, colormap: RwSignal<Colormap>) -> impl IntoView {
    view! {
        column(class = "params") {
            {method(spatial)}
            {grid(spatial)}
            {timing(spatial)}
            {step(spatial)}
            {range(spatial)}
            {band(spatial)}
            {colours(colormap)}
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::SpatialSpectrumNode;

    use super::*;

    #[test]
    fn the_settings_are_read_only_from_a_spatial_body() {
        assert!(spatial_of(&NodeBody::Scope).is_none());
        let mut body = NodeBody::SpatialSpectrum(SpatialSpectrumNode::default());
        assert_eq!(spatial_of(&body), Some(&SpatialSpectrumParams::default()));
        if let Some(params) = spatial_of_mut(&mut body) {
            params.bins = 512;
        }
        assert_eq!(spatial_of(&body).map(|params| params.bins), Some(512));
    }
}
