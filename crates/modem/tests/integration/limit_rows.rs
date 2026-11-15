use sdrmm_modem_test_support::ber::limits::{self, Criterion, LimitRow};

/// The worst value along one impairment axis the link survives without a failed frame.
pub fn axis_row(
    axis: &str,
    unit: &str,
    max_axis: f64,
    tolerance: f64,
    ber_at: impl Fn(f64) -> f64,
) -> LimitRow {
    limits::measure_axis_row(
        axis,
        unit,
        Criterion::FailureBer,
        max_axis,
        tolerance,
        ber_at,
    )
}
