use serde::Serialize;

#[must_use]
pub fn serde_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

#[must_use]
pub fn signed(value: f64, digits: usize) -> String {
    let sign = if value >= 0.0 { "+" } else { "" };
    format!("{sign}{value:.digits$}")
}

#[must_use]
pub fn flag(value: Option<bool>) -> Option<String> {
    value.map(|on| if on { "yes" } else { "no" }.to_owned())
}

#[must_use]
pub fn grouped(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if value < 0 { format!("-{out}") } else { out }
}

#[must_use]
pub fn feet(ft: Option<i32>) -> Option<String> {
    ft.map(|ft| format!("{} ft", grouped(i64::from(ft))))
}

#[must_use]
pub fn knots(kt: Option<f64>) -> Option<String> {
    kt.map(|kt| format!("{kt:.1} kt"))
}

#[must_use]
pub fn degrees(deg: Option<f64>) -> Option<String> {
    deg.map(|deg| format!("{}°", deg.round()))
}

#[must_use]
pub fn position(lat: Option<f64>, lon: Option<f64>) -> Option<String> {
    Some(format!("{:.5}, {:.5}", lat?, lon?))
}

#[must_use]
pub fn hex(value: u64, width: usize) -> String {
    format!("0x{value:0width$X}")
}

#[must_use]
pub fn bare_hex(value: u64, width: usize) -> String {
    format!("{value:0width$X}")
}

#[must_use]
pub fn percent(fraction: f64) -> String {
    format!("{}%", (fraction * 100.0).round())
}

fn unit(value: Option<f64>, digits: usize, symbol: &str) -> Option<String> {
    value.map(|value| format!("{value:.digits$} {symbol}"))
}

#[must_use]
pub fn celsius(value: Option<f32>) -> Option<String> {
    unit(value.map(f64::from), 1, "°C")
}

#[must_use]
pub fn whole_percent(value: Option<f64>) -> Option<String> {
    value.map(|value| format!("{}%", value.round()))
}

#[must_use]
pub fn hectopascal(value: Option<f32>) -> Option<String> {
    unit(value.map(f64::from), 1, "hPa")
}

#[must_use]
pub fn metres_per_second(value: Option<f64>) -> Option<String> {
    unit(value, 1, "m/s")
}

#[must_use]
pub fn climb_rate(value: Option<f64>) -> Option<String> {
    value.map(|value| format!("{} m/s", signed(value, 1)))
}

#[must_use]
pub fn metres(value: Option<f64>) -> Option<String> {
    value.map(|value| format!("{} m", grouped(value.round() as i64)))
}

#[must_use]
pub fn millimetres(value: Option<f32>) -> Option<String> {
    unit(value.map(f64::from), 1, "mm")
}

#[must_use]
pub fn seconds(ms: u32) -> String {
    format!("{:.1} s", f64::from(ms) / 1000.0)
}

#[must_use]
pub fn share(fraction: f32) -> String {
    let digits = usize::from(fraction < 0.1);
    format!("{:.digits$}%", f64::from(fraction) * 100.0)
}

#[must_use]
pub fn join(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_the_way_an_operator_writes_them() {
        assert_eq!(grouped(37_000), "37,000");
        assert_eq!(grouped(-1_088), "-1,088");
        assert_eq!(grouped(999), "999");
        assert_eq!(signed(0.75, 2), "+0.75");
        assert_eq!(signed(-0.2, 1), "-0.2");
        assert_eq!(hex(0x1a, 2), "0x1A");
        assert_eq!(bare_hex(0xa1b2, 5), "0A1B2");
        assert_eq!(
            position(Some(52.52), Some(13.405)).as_deref(),
            Some("52.52000, 13.40500")
        );
        assert_eq!(position(Some(52.52), None), None);
        assert_eq!(
            join(["a".to_owned(), String::new(), "b".to_owned()]),
            "a · b"
        );
    }

    #[test]
    fn weather_reads_with_its_units() {
        assert_eq!(celsius(Some(-3.26)).as_deref(), Some("-3.3 °C"));
        assert_eq!(whole_percent(Some(54.6)).as_deref(), Some("55%"));
        assert_eq!(hectopascal(Some(1013.2)).as_deref(), Some("1013.2 hPa"));
        assert_eq!(climb_rate(Some(5.04)).as_deref(), Some("+5.0 m/s"));
        assert_eq!(climb_rate(Some(-12.0)).as_deref(), Some("-12.0 m/s"));
        assert_eq!(metres(Some(31_234.6)).as_deref(), Some("31,235 m"));
        assert_eq!(millimetres(None), None);
        assert_eq!(seconds(1_260), "1.3 s");
    }

    #[test]
    fn a_small_share_keeps_a_decimal() {
        assert_eq!(share(0.05), "5.0%");
        assert_eq!(share(0.42), "42%");
        assert_eq!(share(1.0), "100%");
    }
}
