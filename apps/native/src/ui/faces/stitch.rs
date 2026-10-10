use sdrmm_wire::{
    array::{ArrayTuningMode, ProcessorGate},
    patch::{NodeBody, PatchGraph},
    processor::{
        ProcessorReading,
        stitch::{STITCH_REPORT_MS, StitchBlend, StitchLane, StitchParams, StitchReading},
    },
};
use zgui::{prelude::*, vocab::SharedString};

use super::processor::{Chip, Processor, age_label, edit, error, footer, head, settings};
use crate::{
    store::Store,
    ui::{
        kit_raster::{button, readout},
        widgets::{check, pick, row_field},
    },
};

pub const LANE_HEADS: [&str; 5] = ["Lane", "MHz", "Eq", "Phase", "Coh"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StitchRow {
    pub lane: String,
    pub mhz: String,
    pub eq: String,
    pub phase: String,
    pub coherence: String,
    pub title: String,
}

impl StitchRow {
    #[must_use]
    pub fn cells(&self) -> [&str; 5] {
        [
            &self.lane,
            &self.mhz,
            &self.eq,
            &self.phase,
            &self.coherence,
        ]
    }
}

#[must_use]
pub fn blends() -> Vec<(StitchBlend, String)> {
    vec![
        (StitchBlend::Snr, "SNR".to_owned()),
        (StitchBlend::Equal, "Equal".to_owned()),
    ]
}

#[must_use]
pub fn blend_title(blend: Option<StitchBlend>) -> &'static str {
    match blend {
        Some(StitchBlend::Snr) => "Favour the cleaner lane in overlaps",
        Some(StitchBlend::Equal) => "Average overlaps",
        None => "Blend",
    }
}

#[must_use]
pub fn mhz(hz: Option<f64>) -> String {
    hz.map_or_else(|| "-".to_owned(), |hz| format!("{:.3} MHz", hz / 1e6))
}

fn signed_db(db: f32) -> String {
    let text = format!("{db:.1}");
    if db > 0.0 && text != "0.0" {
        format!("+{text}")
    } else {
        text
    }
}

#[must_use]
pub fn stitch_row(lane: &StitchLane) -> StitchRow {
    let coherence = lane
        .coherence
        .map_or_else(|| "-".to_owned(), |found| format!("{found:.2}"));
    let phase = format!("{}\u{b0}", lane.phase_deg.round());
    let name = format!("L{}", lane.lane + 1);
    let eq = format!("{} dB", signed_db(lane.noise_eq_db));
    let mut title = vec![name.clone(), eq.clone(), phase.clone(), coherence.clone()];
    if lane.spur_bins > 0 {
        title.push(format!("{} spur bins", lane.spur_bins));
    }
    StitchRow {
        lane: name,
        mhz: format!("{:.3}", lane.center_hz / 1e6),
        eq,
        phase,
        coherence,
        title: title.join(" "),
    }
}

#[must_use]
pub fn summary_rows(reading: Option<&StitchReading>, age: String) -> Vec<(String, String)> {
    vec![
        (
            "Center".to_owned(),
            mhz(reading.map(|found| found.center_hz)),
        ),
        ("Span".to_owned(), mhz(reading.map(|found| found.span_hz))),
        (
            "Lanes".to_owned(),
            reading.map_or(0, |found| found.lanes.len()).to_string(),
        ),
        ("Age".to_owned(), age),
    ]
}

#[must_use]
pub fn spread_chip() -> Chip {
    Chip::danger(
        "Needs spread",
        "Stitch needs the array's lanes tuned side by side",
    )
}

#[must_use]
pub fn stitch_chips(spread: bool, reading: Option<&StitchReading>) -> Vec<Chip> {
    let mut chips = Vec::new();
    if spread {
        chips.push(spread_chip());
    }
    let Some(reading) = reading else {
        return chips;
    };
    if reading.no_overlap {
        chips.push(Chip::warn("No overlap", "Lanes leave gaps between them"));
    }
    if reading.dropped_blocks > 0 {
        chips.push(Chip::danger(
            format!("Drops {}", reading.dropped_blocks),
            "Blocks lost",
        ));
    }
    chips
}

#[must_use]
pub fn needs_spread(graph: &PatchGraph, stitch: &str, gate: Option<ProcessorGate>) -> bool {
    if gate == Some(ProcessorGate::TuningMode) {
        return true;
    }
    graph
        .array_of_processor(stitch)
        .and_then(|array| graph.node(array))
        .is_some_and(|found| {
            matches!(&found.body, NodeBody::Array(array) if array.tuning != ArrayTuningMode::Spread)
        })
}

pub fn spread_array(graph: &mut PatchGraph, stitch: &str) -> bool {
    let Some(array) = graph.array_of_processor(stitch).map(str::to_owned) else {
        return false;
    };
    let found = graph
        .nodes
        .iter_mut()
        .find(|found| found.id == array)
        .map(|found| &mut found.body);
    match found {
        Some(NodeBody::Array(array)) => {
            array.tuning = ArrayTuningMode::Spread;
            true
        }
        _ => false,
    }
}

fn read(body: &NodeBody) -> Option<&StitchParams> {
    match body {
        NodeBody::Stitch(node) => Some(&node.settings),
        _ => None,
    }
}

fn write(body: &mut NodeBody) -> Option<&mut StitchParams> {
    match body {
        NodeBody::Stitch(node) => Some(&mut node.settings),
        _ => None,
    }
}

fn heard(reading: &ProcessorReading) -> Option<&StitchReading> {
    match reading {
        ProcessorReading::Stitch(found) => Some(found),
        _ => None,
    }
}

fn titled(title: impl Into<SharedString>, body: impl IntoView + 'static) -> impl IntoView {
    let title = title.into();
    view! { box(a11y:description = title) {{body}} }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install_stylesheet("stitch-face", SHEET);
    let processor = Processor::new(store, node.clone());
    let params = settings(store, node.clone(), read);
    let reading = processor.reading(heard);
    let period = Signal::derive(|| STITCH_REPORT_MS);
    let subtitle = processor.subtitle(period);
    let stale = processor.stale(period);
    let age = processor.age();
    let gate = processor.gate();
    let health = processor.health;
    let spread = {
        let node = node.clone();
        Memo::new(move |_| {
            store
                .graph
                .with(|graph| needs_spread(graph, &node, gate.get()))
        })
    };
    let rows = move || reading.with(|found| summary_rows(found.as_ref(), age_label(age.get())));
    let chips = move || reading.with(|found| stitch_chips(spread.get(), found.as_ref()));
    view! {
        column(class = "face") {
            {head(subtitle)}
            column(class = "st__live", class:dim = move || stale.get()) {
                {readout(rows)}
                {lane_table(reading)}
            }
            {controls(store, node.clone(), params)}
            {error(health)}
            {spread_button(store, node, spread)}
            {footer(health, chips)}
        }
    }
}

fn lane_table(reading: Memo<Option<StitchReading>>) -> impl IntoView {
    move || {
        let rows: Vec<StitchRow> = reading.with(|found| {
            found
                .as_ref()
                .map(|found| found.lanes.iter().map(stitch_row).collect())
                .unwrap_or_default()
        });
        (!rows.is_empty()).then(|| {
            let heads = LANE_HEADS
                .iter()
                .map(|head| view! { text(class = "st__cell st__head") {{*head}} })
                .collect::<Vec<_>>();
            let body = rows.into_iter().map(lane_row).collect::<Vec<_>>();
            view! {
                column(class = "st__table") {
                    row(class = "st__row") {{heads}}
                    {body}
                }
            }
        })
    }
}

fn lane_row(row: StitchRow) -> AnyView {
    let cells = row
        .cells()
        .iter()
        .map(|cell| {
            let cell = (*cell).to_owned();
            view! { text(class = "st__cell") {{cell}} }
        })
        .collect::<Vec<_>>();
    AnyView::new(view! {
        row(class = "st__row", a11y:description = SharedString::from(row.title)) {{cells}}
    })
}

fn toggle(
    params: Memo<Option<StitchParams>>,
    change: impl Fn(Box<dyn FnOnce(&mut StitchParams)>) + 'static,
    label: &'static str,
    title: &'static str,
    field: fn(&mut StitchParams) -> &mut bool,
) -> impl IntoView {
    let on = Signal::derive(move || {
        params.with(|found| {
            found
                .map(|mut found| *field(&mut found))
                .unwrap_or_default()
        })
    });
    titled(
        title,
        row_field(
            label,
            check(on, move |next| {
                change(Box::new(move |found| *field(found) = next));
            }),
        ),
    )
}

fn controls(store: Store, node: String, params: Memo<Option<StitchParams>>) -> impl IntoView {
    let change = move |apply: Box<dyn FnOnce(&mut StitchParams)>| edit(store, &node, write, apply);
    let blend = Signal::derive(move || params.with(|found| found.map(|found| found.blend)));
    let on_blend = {
        let change = change.clone();
        move |picked: StitchBlend| change(Box::new(move |found| found.blend = picked))
    };
    view! {
        column(class = "params") {
            box(a11y:description = move || SharedString::from(blend_title(blend.get()))) {
                {row_field("Blend", pick(blends(), blend, on_blend))}
            }
            {toggle(params, change.clone(), "Equalise", "Match noise floors", |found| &mut found.noise_equalise)}
            {toggle(params, change.clone(), "Flatten", "Undo each lane's filter droop", |found| &mut found.flatten)}
            {toggle(params, change.clone(), "Spurs", "Drop lane spurs in overlaps", |found| &mut found.spur_reject)}
            {toggle(params, change, "Phase", "Line up phase across seams", |found| &mut found.match_phase)}
        }
    }
}

fn spread_button(store: Store, node: String, spread: Memo<bool>) -> impl IntoView {
    move || {
        let node = node.clone();
        spread.get().then(|| {
            let press = move || {
                let node = node.clone();
                store.edit_graph(move |graph| {
                    if !spread_array(graph, &node) {
                        store.say("No array wired to spread");
                    }
                });
            };
            view! {
                row(class = "section") {
                    {titled("Tune the wired array's lanes side by side", button("Spread array", Signal::derive(|| false), press))}
                }
            }
        })
    }
}

const SHEET: &str = css!(
    r#"
.st__live { gap: 6px; }
.st__live.dim { opacity: 0.5; }
.st__table { padding-top: 4px; border-top: 1px solid var(--line); }
.st__row { gap: 6px; }
.st__cell { flex: 1 1 0; min-width: 0; font-family: var(--mono); font-size: 11px; color: var(--ink); }
.st__head { font-size: 9px; letter-spacing: 0.09em; text-transform: uppercase; color: var(--ink-faint); }
"#
);

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        array::ArrayNode,
        patch::{ARRAY_PORT, PatchEdge, PatchNode, PortRef, Position, StitchNode},
    };

    use super::*;

    fn lane() -> StitchLane {
        StitchLane {
            lane: 1,
            center_hz: 101.25e6,
            noise_eq_db: 1.26,
            coherence: Some(0.876),
            phase_deg: -12.4,
            spur_bins: 0,
        }
    }

    fn placed(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn wired(tuning: ArrayTuningMode) -> PatchGraph {
        PatchGraph {
            nodes: vec![
                placed(
                    "array",
                    NodeBody::Array(ArrayNode {
                        tuning,
                        ..ArrayNode::default()
                    }),
                ),
                placed("stitch", NodeBody::Stitch(StitchNode::default())),
            ],
            edges: vec![PatchEdge {
                from: PortRef {
                    node: "array".to_owned(),
                    port: ARRAY_PORT.to_owned(),
                },
                to: PortRef {
                    node: "stitch".to_owned(),
                    port: ARRAY_PORT.to_owned(),
                },
            }],
        }
    }

    #[test]
    fn a_lane_row_reads_name_centre_eq_phase_and_coherence() {
        let row = stitch_row(&lane());
        assert_eq!(
            row.cells(),
            ["L2", "101.250", "+1.3 dB", "-12\u{b0}", "0.88"]
        );
        assert_eq!(row.title, "L2 +1.3 dB -12\u{b0} 0.88");
    }

    #[test]
    fn spurs_and_missing_coherence_show_in_the_row() {
        let row = stitch_row(&StitchLane {
            coherence: None,
            spur_bins: 3,
            noise_eq_db: -0.04,
            ..lane()
        });
        assert_eq!(row.coherence, "-");
        assert_eq!(row.eq, "-0.0 dB");
        assert!(row.title.ends_with("3 spur bins"));
        let flat = stitch_row(&StitchLane {
            noise_eq_db: 0.01,
            ..lane()
        });
        assert_eq!(flat.eq, "0.0 dB");
    }

    #[test]
    fn the_summary_shows_dashes_before_the_first_reading() {
        let rows = summary_rows(None, "-".to_owned());
        assert_eq!(rows[0], ("Center".to_owned(), "-".to_owned()));
        assert_eq!(rows[2], ("Lanes".to_owned(), "0".to_owned()));
        let reading = StitchReading {
            center_hz: 100e6,
            span_hz: 7.5e6,
            lanes: vec![lane(), lane()],
            ..StitchReading::default()
        };
        let rows = summary_rows(Some(&reading), "1.0 s".to_owned());
        assert_eq!(rows[0].1, "100.000 MHz");
        assert_eq!(rows[1].1, "7.500 MHz");
        assert_eq!(rows[2].1, "2");
    }

    #[test]
    fn gaps_and_lost_blocks_become_chips() {
        assert!(stitch_chips(false, None).is_empty());
        let reading = StitchReading {
            no_overlap: true,
            dropped_blocks: 4,
            ..StitchReading::default()
        };
        let chips = stitch_chips(true, Some(&reading));
        let labels: Vec<&str> = chips.iter().map(|chip| chip.label.as_str()).collect();
        assert_eq!(labels, vec!["Needs spread", "No overlap", "Drops 4"]);
        assert!(chips[0].danger);
        assert!(!chips[1].danger);
        assert!(chips[2].danger);
    }

    #[test]
    fn stitch_needs_an_array_tuned_side_by_side() {
        assert!(needs_spread(
            &wired(ArrayTuningMode::Together),
            "stitch",
            None
        ));
        assert!(!needs_spread(
            &wired(ArrayTuningMode::Spread),
            "stitch",
            None
        ));
        assert!(needs_spread(
            &wired(ArrayTuningMode::Spread),
            "stitch",
            Some(ProcessorGate::TuningMode)
        ));
        assert!(!needs_spread(&PatchGraph::default(), "stitch", None));
    }

    #[test]
    fn spreading_retunes_the_wired_array() {
        let mut graph = wired(ArrayTuningMode::Together);
        assert!(spread_array(&mut graph, "stitch"));
        assert!(!needs_spread(&graph, "stitch", None));
        assert!(!spread_array(&mut PatchGraph::default(), "stitch"));
    }

    #[test]
    fn blends_carry_the_web_labels_and_titles() {
        assert_eq!(blends()[0].1, "SNR");
        assert_eq!(blend_title(Some(StitchBlend::Equal)), "Average overlaps");
        assert!(StitchParams::default().valid());
    }
}
