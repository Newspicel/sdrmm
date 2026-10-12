use super::{
    EventPayload, Field, Presentation, Unit, count, fields, joined, number, percent, text, text_of,
};
use crate::{
    decode::{EventKind, IdentReport, IdentSignal, Modulation, ProtocolMatch},
    units::hertz,
};

impl EventPayload for IdentReport {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            fields: ident_fields(self),
            ..Presentation::of(ident_summary(self))
        }
    }
}

fn ident_summary(r: &IdentReport) -> String {
    let Some(loudest) = r.loudest() else {
        return Modulation::None.label().to_owned();
    };
    let mut parts = Vec::new();
    if r.signals.len() > 1 {
        parts.push(format!("{} signals", r.signals.len()));
    }
    parts.push(loudest.modulation.label().to_owned());
    parts.push(hertz(loudest.bandwidth_hz));
    parts.extend(loudest.symbol_rate_hz.map(|baud| format!("{baud:.0} Bd")));
    parts.extend(
        loudest
            .deviation_hz
            .map(|deviation| format!("±{deviation:.0} Hz")),
    );
    parts.extend(
        loudest
            .burst_ms
            .map(|burst| format!("{burst:.1} ms bursts")),
    );
    parts.push(format!("{:.0} dB SNR", loudest.snr_db));
    parts.extend(
        loudest
            .best()
            .map(|best| format!("{} ({})", best.name, score(best))),
    );
    joined(parts)
}

fn score(candidate: &ProtocolMatch) -> String {
    if candidate.confirmed {
        "confirmed".to_owned()
    } else {
        format!("{:.0}%", candidate.score * 100.0)
    }
}

fn ident_fields(r: &IdentReport) -> Vec<Field> {
    let Some(loudest) = r.loudest() else {
        return fields(&[
            ("Modulation", text_of(Modulation::None.label())),
            ("Loudest bin", number(Some(r.snr_db), Unit::Db, 1)),
        ]);
    };
    let mut shown = fields(&[
        ("Signals", count(Some(r.signals.len() as f64))),
        ("Frequency", number(Some(loudest.frequency_hz), Unit::Hz, 0)),
        ("Modulation", text_of(modulation_label(loudest))),
        ("Confidence", percent(loudest.confidence)),
    ]);
    shown.extend(signal_fields(loudest));
    shown.extend(fields(&[("Candidates", text(Some(&candidates(r))))]));
    shown
}

fn signal_fields(s: &IdentSignal) -> Vec<Field> {
    let levels = Some(s.features.frequency_levels).filter(|levels| *levels > 1);
    let duty = Some(s.features.duty).filter(|duty| *duty < 0.99);
    fields(&[
        ("Bandwidth", number(Some(s.bandwidth_hz), Unit::Hz, 0)),
        ("Off tune", number(Some(s.center_offset_hz), Unit::Hz, 0)),
        ("SNR", number(Some(s.snr_db), Unit::Db, 1)),
        ("Symbol rate", number(s.symbol_rate_hz, Unit::Baud, 0)),
        ("Deviation", number(s.deviation_hz, Unit::Hz, 0)),
        (
            "Burst",
            number(s.burst_ms.map(|ms| ms / 1_000.0), Unit::Seconds, 5),
        ),
        (
            "Burst period",
            number(s.burst_period_ms.map(|ms| ms / 1_000.0), Unit::Seconds, 4),
        ),
        (
            "OFDM symbol",
            number(s.ofdm_symbol_us.map(|us| us / 1e6), Unit::Seconds, 6),
        ),
        (
            "OFDM guard",
            number(s.ofdm_guard_us.map(|us| us / 1e6), Unit::Seconds, 6),
        ),
        ("Duty", duty.and_then(percent)),
        ("Frequency levels", count(levels)),
    ])
}

fn candidates(r: &IdentReport) -> String {
    r.signals
        .iter()
        .map(|signal| {
            let mut lines = vec![format!(
                "{} · {}",
                hertz(signal.frequency_hz),
                modulation_label(signal)
            )];
            lines.extend(signal.candidates.iter().map(|candidate| {
                format!(
                    "{}: {}, {}",
                    candidate.name,
                    score(candidate),
                    candidate.why
                )
            }));
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn modulation_label(signal: &IdentSignal) -> String {
    let base = signal.modulation.label();
    match signal.sideband {
        Some(sideband) => format!("{base} ({})", sideband.label()),
        None => base.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_report_names_the_loudest_bin() {
        let view = IdentReport {
            snr_db: 4.25,
            signals: Vec::new(),
        }
        .present(EventKind::Ident);
        assert_eq!(view.labels(), ["Modulation", "Loudest bin"]);
        assert_eq!(view.field("Modulation"), text_of("no signal").as_ref());
    }

    #[test]
    fn the_loudest_signal_leads_and_candidates_close() {
        let report: IdentReport = serde_json::from_value(serde_json::json!({
            "snr_db": 30.0,
            "signals": [{
                "modulation": "fsk2", "confidence": 0.82, "frequency_hz": 433.92e6,
                "center_offset_hz": 1200.0, "bandwidth_hz": 25_000.0, "snr_db": 21.5,
                "symbol_rate_hz": 4800.0, "deviation_hz": 9600.0,
                "candidates": [{ "name": "POCSAG", "score": 0.7, "why": "rate fits" }],
                "features": {
                    "envelope_variation": 0.0, "duty": 0.5, "keying_depth_db": 0.0,
                    "spectral_asymmetry": 0.0, "carrier_db": 0.0, "spectral_flatness": 0.0,
                    "frequency_levels": 2, "frequency_spread_hz": 0.0, "square_line_db": 0.0,
                    "quartic_line_db": 0.0,
                },
            }],
        }))
        .unwrap();
        let view = report.present(EventKind::Ident);
        assert_eq!(view.field("Signals"), count(Some(1)).as_ref());
        assert_eq!(view.field("Modulation"), text_of("2-FSK").as_ref());
        assert_eq!(view.field("Confidence"), percent(0.82).as_ref());
        assert_eq!(
            view.field("Symbol rate"),
            number(Some(4800.0), Unit::Baud, 0).as_ref()
        );
        assert_eq!(view.field("Duty"), percent(0.5).as_ref());
        assert_eq!(view.field("Frequency levels"), count(Some(2)).as_ref());
        assert_eq!(
            view.field("Candidates"),
            text_of("433.92 MHz · 2-FSK\nPOCSAG: 70%, rate fits").as_ref()
        );
    }
}
