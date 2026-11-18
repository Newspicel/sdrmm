//! Readout and face helpers every processor's presenters share.

use sdrmm_wire::channel::ChannelHealth;
use sdrmm_wire::present::{Field, FieldValue, Unit, number, text_of};
use sdrmm_wire::processor::view::Flag;

pub(crate) fn degrees(value: Option<f32>) -> Option<FieldValue> {
    number(value.map(|deg| deg.rem_euclid(360.0)), Unit::Degrees, 0)
}

pub(crate) fn db(value: Option<f32>, digits: u8) -> Option<FieldValue> {
    number(value, Unit::Db, digits)
}

pub(crate) fn snr_health(snr_db: Option<f32>) -> ChannelHealth {
    snr_db.map_or_else(ChannelHealth::default, |db| {
        ChannelHealth::default().snr(db)
    })
}

/// The readouts of a peak past the first, keyed by its rank.
pub(crate) fn ranked<T>(peaks: &[T], text: impl Fn(&T) -> String) -> Vec<Field> {
    peaks
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(index, peak)| {
            let rank = index + 1;
            text_of(text(peak))
                .map(|value| Field::new(format!("peak_{rank}"), format!("#{rank}"), value))
        })
        .collect()
}

pub(crate) fn flags(rules: &[(bool, &str, &str)]) -> Vec<Flag> {
    rules
        .iter()
        .filter(|(on, ..)| *on)
        .map(|(_, label, title)| Flag::warn(*label, *title))
        .collect()
}

pub(crate) fn drops(count: u64, title: &str) -> Option<Flag> {
    (count > 0).then(|| Flag::danger(format!("Drops {count}"), title))
}

pub(crate) fn signed(value: f32, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let zero = text.parse::<f32>().is_ok_and(|parsed| parsed == 0.0);
    if value > 0.0 && !zero {
        format!("+{text}")
    } else {
        text
    }
}

pub(crate) fn wrap_deg(deg: f32) -> f32 {
    ((deg * 10.0).round() / 10.0).rem_euclid(360.0)
}

pub(crate) fn angle_gap(a: f32, b: f32) -> f32 {
    let gap = (wrap_deg(a) - wrap_deg(b)).abs();
    gap.min(360.0 - gap)
}
