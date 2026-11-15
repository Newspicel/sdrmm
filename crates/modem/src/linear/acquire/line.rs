const MIN_DRIFT_SIGMAS: f64 = 4.0;

/// One segment's tone: where it sits in symbols from the block centre, and how strongly.
#[derive(Clone, Copy, Debug)]
pub(super) struct SegmentTone {
    pub(super) at_symbol: f64,
    pub(super) cycles_per_symbol: f64,
    pub(super) weight: f64,
}

/// Weighted least-squares `(intercept, slope)` of tone frequency against time. A slope within
/// its own noise reads as none.
pub(super) fn fit_line(segments: &[SegmentTone]) -> Option<(f64, f64)> {
    if segments.len() < 2 {
        return None;
    }
    let w: f64 = segments.iter().map(|s| s.weight).sum();
    let t = segments.iter().map(|s| s.weight * s.at_symbol).sum::<f64>() / w;
    let f = segments
        .iter()
        .map(|s| s.weight * s.cycles_per_symbol)
        .sum::<f64>()
        / w;
    let stt: f64 = segments
        .iter()
        .map(|s| s.weight * (s.at_symbol - t).powi(2))
        .sum();
    if stt <= 0.0 {
        return None;
    }
    let stf: f64 = segments
        .iter()
        .map(|s| s.weight * (s.at_symbol - t) * (s.cycles_per_symbol - f))
        .sum();
    let slope = stf / stt;
    let intercept = f - slope * t;
    if segments.len() > 2 {
        let scatter: f64 = segments
            .iter()
            .map(|s| s.weight * (s.cycles_per_symbol - intercept - slope * s.at_symbol).powi(2))
            .sum::<f64>()
            / w
            * segments.len() as f64
            / (segments.len() - 2) as f64;
        let standard_error = (scatter * w / segments.len() as f64 / stt).sqrt();
        if slope.abs() < MIN_DRIFT_SIGMAS * standard_error {
            return Some((f, 0.0));
        }
    }
    Some((intercept, slope))
}
