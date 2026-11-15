/// One event as a line a person reads: time, kind, frequency, station, summary. Line breaks
/// and tabs inside a field become spaces so every event stays on its own line.
pub(crate) fn line(
    at: &str,
    kind: &str,
    freq_hz: f64,
    station: Option<&str>,
    summary: &str,
) -> String {
    let freq = format!("{:.6} MHz", freq_hz / 1e6);
    let mut fields = vec![flat(at), flat(kind), freq];
    if let Some(station) = station.filter(|station| !station.trim().is_empty()) {
        fields.push(flat(station));
    }
    fields.push(flat(summary));
    let mut line = fields.join("  ");
    line.push('\n');
    line
}

fn flat(value: &str) -> String {
    value
        .split(['\n', '\r', '\t'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
