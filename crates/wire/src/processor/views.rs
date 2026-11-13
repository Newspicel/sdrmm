use crate::processor::beamformer::BeamformerReading;
use crate::processor::correlator::{Baseline, CorrelatorReading};
use crate::processor::df::{DfPeak, DfReading};
use crate::processor::plugin::PluginReading;
use crate::processor::polarimeter::PolarimeterReading;
use crate::processor::spatial::SpatialReading;
use crate::processor::stitch::{StitchLane, StitchReading};
use crate::processor::view::{
    BearingView, Cell, EllipseView, Flag, Headline, Layer, Mark, MarkShape, Needle, NeedleWeight,
    PatternView, ProcessorView, SurfaceView, TableRow, TableView,
};
use crate::radar::{RadarAoa, RadarProblem, RadarTrack, RadarUpdate, TrackState};

const NULL_CLOSE_DEG: f32 = 10.0;
const TRACK_ROWS: usize = 6;
const TRACK_HEADS: [&str; 6] = ["Track", "km", "m/s", "Bearing", "dB", "Truth"];
const LANE_HEADS: [&str; 5] = ["Lane", "MHz", "Eq", "Phase", "Coh"];
const BASELINE_TITLE: &str = "Coherence, phase, delay, SNR";

fn flags(rules: &[(bool, &str, &str)]) -> Vec<Flag> {
    rules
        .iter()
        .filter(|(on, ..)| *on)
        .map(|(_, label, title)| Flag::warn(*label, *title))
        .collect()
}

fn drops(count: u64, title: &str) -> Option<Flag> {
    (count > 0).then(|| Flag::danger(format!("Drops {count}"), title))
}

fn signed(value: f32, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let zero = text.parse::<f32>().is_ok_and(|parsed| parsed == 0.0);
    if value > 0.0 && !zero {
        format!("+{text}")
    } else {
        text
    }
}

fn wrap_deg(deg: f32) -> f32 {
    ((deg * 10.0).round() / 10.0).rem_euclid(360.0)
}

fn angle_gap(a: f32, b: f32) -> f32 {
    let gap = (wrap_deg(a) - wrap_deg(b)).abs();
    gap.min(360.0 - gap)
}

fn df_needles(peaks: &[DfPeak]) -> Vec<Needle> {
    let mut needles = Vec::with_capacity(peaks.len() * 2);
    for (rank, peak) in peaks.iter().enumerate() {
        needles.push(Needle {
            relative_deg: peak.relative_deg,
            true_deg: peak.true_deg,
            weight: if rank == 0 {
                NeedleWeight::Primary
            } else {
                NeedleWeight::Secondary
            },
        });
        if let Some(mirror) = peak.mirror_deg {
            needles.push(Needle {
                relative_deg: mirror,
                true_deg: peak.mirror_true_deg,
                weight: NeedleWeight::Mirror,
            });
        }
    }
    needles
}

impl DfReading {
    #[must_use]
    pub fn view(&self) -> ProcessorView {
        ProcessorView {
            flags: flags(&[
                (self.squelched, "Squelch", "Peak below squelch"),
                (
                    self.azimuth_deg.is_none(),
                    "No heading",
                    "Wire a GPS with heading",
                ),
                (
                    self.station.is_none(),
                    "No position",
                    "Wire a GPS to the array to send bearings",
                ),
                (
                    self.aliasing,
                    "Aliased",
                    "Elements more than half a wavelength apart",
                ),
                (
                    self.mode_aliasing,
                    "Mode alias",
                    "Too high for phase modes on this circle",
                ),
                (
                    self.mirror,
                    "Mirror",
                    "A line array cannot tell front from back",
                ),
                (
                    self.rotating,
                    "Rotating",
                    "Turning too fast, blocks skipped",
                ),
                (
                    self.singular,
                    "Singular",
                    "Covariance could not be inverted",
                ),
                (
                    self.table_out_of_range,
                    "Table off",
                    "Outside the calibration table",
                ),
            ]),
            bearing: Some(BearingView {
                spectrum: self.pseudospectrum.clone(),
                heading_deg: self.azimuth_deg,
                needles: df_needles(&self.peaks),
                sigma_deg: self.peaks.first().map(|peak| peak.sigma_deg),
            }),
            ..ProcessorView::default()
        }
    }
}

impl BeamformerReading {
    fn null_close(&self) -> bool {
        self.steer_deg.is_some_and(|steer| {
            self.nulls_deg
                .iter()
                .any(|deg| angle_gap(*deg, steer) < NULL_CLOSE_DEG)
        })
    }

    #[must_use]
    pub fn view(&self) -> ProcessorView {
        let reset = format!("Weights restarted {} times", self.resets);
        ProcessorView {
            flags: flags(&[
                (self.no_steer, "No steer", "No bearing to steer at"),
                (self.steer_stale, "Stale", "Last bearing is old"),
                (self.singular, "Singular", "Lanes too alike to solve"),
                (self.resets > 0 || self.diverged, "Reset", &reset),
                (self.band_full, "Band full", "Band wider than the lanes"),
                (
                    self.null_close(),
                    "Null close",
                    "A null sits next to the steer",
                ),
            ]),
            pattern: Some(PatternView {
                pattern: self.pattern.clone(),
                steer_deg: self.steer_deg,
                nulls_deg: self.nulls_deg.clone(),
                weights: self.weights.clone(),
            }),
            ..ProcessorView::default()
        }
    }
}

impl PolarimeterReading {
    #[must_use]
    pub fn view(&self) -> ProcessorView {
        ProcessorView {
            ellipse: Some(EllipseView {
                angle_deg: self.angle_deg,
                ellipticity_deg: self.ellipticity_deg,
                v: self.v,
            }),
            ..ProcessorView::default()
        }
    }
}

impl SpatialReading {
    #[must_use]
    pub fn view(&self) -> ProcessorView {
        ProcessorView {
            flags: drops(self.dropped_frames, "Frames lost")
                .into_iter()
                .collect(),
            surface: Some(SurfaceView {
                marks: self
                    .peaks
                    .iter()
                    .map(|peak| Mark {
                        x: peak.freq_hz,
                        y: f64::from(peak.bearing_deg),
                        true_y: peak.true_deg.map(f64::from),
                        shape: MarkShape::Cross,
                        title: Some(format!("{:.0} dB", peak.db)),
                        ..Mark::default()
                    })
                    .collect(),
                heading_deg: self.azimuth_deg,
                ..SurfaceView::default()
            }),
            ..ProcessorView::default()
        }
    }
}

fn baseline_layer(baseline: &Baseline) -> Layer {
    Layer {
        label: format!("{}-{}", baseline.a + 1, baseline.b + 1),
        text: format!(
            "{:.2} ∠{:.0}° {:.1} ns {:.0} dB",
            baseline.coherence, baseline.phase_deg, baseline.delay_ns, baseline.snr_db
        ),
        title: BASELINE_TITLE.to_owned(),
        phase_deg: Some(baseline.phase_deg),
    }
}

impl CorrelatorReading {
    #[must_use]
    pub fn view(&self) -> ProcessorView {
        ProcessorView {
            surface: Some(SurfaceView {
                layers: self.baselines.iter().map(baseline_layer).collect(),
                ..SurfaceView::default()
            }),
            ..ProcessorView::default()
        }
    }
}

fn lane_row(lane: &StitchLane) -> TableRow {
    let name = format!("L{}", lane.lane + 1);
    let eq = format!("{} dB", signed(lane.noise_eq_db, 1));
    let phase = format!("{:.0}°", lane.phase_deg);
    let coherence = lane
        .coherence
        .map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"));
    let mut title = format!("{name} {eq} {phase} {coherence}");
    if lane.spur_bins > 0 {
        title.push_str(&format!(" {} spur bins", lane.spur_bins));
    }
    TableRow {
        key: lane.lane,
        cells: vec![
            Cell::of(name),
            Cell::of(format!("{:.3}", lane.center_hz / 1e6)),
            Cell::of(eq),
            Cell::of(phase),
            Cell::of(coherence),
        ],
        title: Some(title),
        ..TableRow::default()
    }
}

impl StitchReading {
    #[must_use]
    pub fn view(&self) -> ProcessorView {
        let mut found = flags(&[(
            self.no_overlap,
            "No overlap",
            "Lanes leave gaps between them",
        )]);
        found.extend(drops(self.dropped_blocks, "Blocks lost"));
        ProcessorView {
            flags: found,
            table: Some(TableView {
                heads: LANE_HEADS.map(str::to_owned).to_vec(),
                rows: self.lanes.iter().map(lane_row).collect(),
            }),
            ..ProcessorView::default()
        }
    }
}

const fn chip_only(problem: &RadarProblem) -> Option<&'static str> {
    match problem {
        RadarProblem::TableOutOfRange => Some("Bearings use the ideal geometry"),
        _ => None,
    }
}

fn bearing_cell(aoa: Option<&RadarAoa>) -> Cell {
    match aoa {
        None => Cell::of("-"),
        Some(RadarAoa {
            bearing_deg: Some(deg),
            ..
        }) => Cell::of(format!("{deg:.0}°")),
        Some(aoa) => Cell {
            text: format!("{:.0}°", aoa.azimuth_deg),
            faint: true,
            title: Some("Array frame".to_owned()),
        },
    }
}

fn truth_text(track: &RadarTrack) -> String {
    track.truth.as_ref().map_or_else(
        || "-".to_owned(),
        |truth| truth.callsign.clone().unwrap_or_else(|| truth.id.clone()),
    )
}

fn track_row(track: &RadarTrack) -> TableRow {
    let coasting = track.state == TrackState::Coasting;
    TableRow {
        key: track.id,
        cells: vec![
            Cell::of(format!("T{}", track.id)),
            Cell::of(format!("{:.1}", track.range_km)),
            Cell::of(format!("{:.0}", track.range_rate_mps)),
            bearing_cell(track.aoa.as_ref()),
            Cell::of(format!("{:.0}", track.snr_db)),
            Cell::of(truth_text(track)),
        ],
        title: Some(
            if coasting {
                "Coasting"
            } else {
                "Show on the plot"
            }
            .to_owned(),
        ),
        dim: coasting,
        pick: true,
    }
}

fn track_mark(track: &RadarTrack) -> Mark {
    Mark {
        x: f64::from(track.range_km),
        y: f64::from(track.doppler_hz),
        shape: MarkShape::Square,
        key: Some(track.id),
        label: Some(format!("T{}", track.id)),
        trail: track
            .trail
            .iter()
            .map(|point| [f64::from(point.range_km), f64::from(point.doppler_hz)])
            .collect(),
        faint: track.state == TrackState::Coasting,
        ..Mark::default()
    }
}

impl RadarUpdate {
    fn marks(&self) -> Vec<Mark> {
        let detections = self.detections.iter().map(|detection| Mark {
            x: f64::from(detection.range_km),
            y: f64::from(detection.doppler_hz),
            ..Mark::default()
        });
        let truth = self
            .truth
            .iter()
            .filter(|truth| truth.in_view)
            .map(|truth| Mark {
                x: f64::from(truth.range_km),
                y: f64::from(truth.doppler_hz),
                shape: MarkShape::Triangle,
                title: Some(truth.callsign.clone().unwrap_or_else(|| truth.id.clone())),
                ..Mark::default()
            });
        detections
            .chain(self.tracks.iter().map(track_mark))
            .chain(truth)
            .collect()
    }

    fn headline(&self) -> Option<Headline> {
        if let Some(problem) = self.problems.iter().find(|held| chip_only(held).is_none()) {
            return Some(Headline {
                text: problem.label().to_owned(),
                warn: true,
            });
        }
        (self.axes.carrier_hz > 0.0).then(|| Headline {
            text: format!("{:.2} MHz", self.axes.carrier_hz / 1e6),
            warn: false,
        })
    }

    fn tracks_by_strength(&self) -> Vec<&RadarTrack> {
        let mut tracks: Vec<&RadarTrack> = self.tracks.iter().collect();
        tracks.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db).then(a.id.cmp(&b.id)));
        tracks.truncate(TRACK_ROWS);
        tracks
    }

    #[must_use]
    pub fn view(&self) -> ProcessorView {
        ProcessorView {
            headline: self.headline(),
            flags: self
                .problems
                .iter()
                .filter_map(|problem| {
                    chip_only(problem).map(|title| Flag::warn(problem.label(), title))
                })
                .collect(),
            surface: Some(SurfaceView {
                marks: self.marks(),
                ..SurfaceView::default()
            }),
            table: Some(TableView {
                heads: TRACK_HEADS.map(str::to_owned).to_vec(),
                rows: self
                    .tracks_by_strength()
                    .into_iter()
                    .map(track_row)
                    .collect(),
            }),
            ..ProcessorView::default()
        }
    }
}

impl PluginReading {
    #[must_use]
    pub fn view(&self) -> ProcessorView {
        ProcessorView::default()
    }
}

#[cfg(test)]
mod tests;
