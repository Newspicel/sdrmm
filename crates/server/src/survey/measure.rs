use sdrmm_wire::{
    SurveyCell,
    survey::{MAX_SURVEY_CELLS, cell_key},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Merged {
    Added(usize),
    Updated(usize),
    Evicted(usize),
}

impl Merged {
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Added(index) | Self::Updated(index) | Self::Evicted(index) => index,
        }
    }
}

pub(crate) fn spans(center_hz: f64, span_hz: f64, target_hz: f64) -> bool {
    (target_hz - center_hz).abs() <= span_hz / 2.0
}

pub(crate) fn measure_dbfs(
    center_hz: f64,
    span_hz: f64,
    db: &[f32],
    target_hz: f64,
    bandwidth_hz: f64,
) -> Option<f32> {
    let count = db.len();
    if count == 0
        || span_hz.is_nan()
        || span_hz <= 0.0
        || !center_hz.is_finite()
        || !target_hz.is_finite()
        || !bandwidth_hz.is_finite()
        || bandwidth_hz <= 0.0
    {
        return None;
    }
    if !spans(center_hz, span_hz, target_hz) {
        return None;
    }
    let low = center_hz - span_hz / 2.0;
    let high = center_hz + span_hz / 2.0;
    let bin_hz = span_hz / count as f64;
    let last_bin = count as f64 - 1.0;
    let slice_low = low.max(target_hz - bandwidth_hz / 2.0);
    let slice_high = high.min(target_hz + bandwidth_hz / 2.0);
    let first = ((slice_low - low) / bin_hz).floor().clamp(0.0, last_bin);
    let last = (((slice_high - low) / bin_hz).ceil() - 1.0).clamp(first, last_bin);
    db[first as usize..=last as usize]
        .iter()
        .copied()
        .filter(|level| !level.is_nan())
        .reduce(f32::max)
}

fn db_to_power(db: f32) -> f64 {
    10f64.powf(f64::from(db) / 10.0)
}

pub(crate) fn merge(cells: &mut Vec<SurveyCell>, incoming: SurveyCell) -> Merged {
    let key = cell_key(incoming.latitude, incoming.longitude, incoming.frequency_hz);
    let found = cells
        .iter()
        .position(|cell| cell_key(cell.latitude, cell.longitude, cell.frequency_hz) == key);
    let Some(index) = found else {
        cells.push(SurveyCell {
            observations: 1,
            ..incoming
        });
        if cells.len() > MAX_SURVEY_CELLS {
            cells.remove(0);
            return Merged::Evicted(cells.len() - 1);
        }
        return Merged::Added(cells.len() - 1);
    };
    let cell = &mut cells[index];
    let before = f64::from(cell.observations);
    let observations = cell.observations.saturating_add(1);
    let after = f64::from(observations);
    let power = (db_to_power(cell.level_dbfs) * before + db_to_power(incoming.level_dbfs)) / after;
    cell.level_dbfs = (10.0 * power.log10()) as f32;
    cell.latitude = (cell.latitude * before + incoming.latitude) / after;
    cell.longitude = (cell.longitude * before + incoming.longitude) / after;
    cell.frequency_hz = incoming.frequency_hz;
    cell.measured_at = incoming.measured_at;
    cell.accuracy_m = incoming.accuracy_m;
    cell.observations = observations;
    Merged::Updated(index)
}
