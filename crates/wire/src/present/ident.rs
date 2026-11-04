use super::{
    EventPayload, Field, Presentation, Unit, count, fields, joined, number, percent, text, text_of,
};
use crate::{
    decode::{EventKind, IdentReport, IdentSignal, Modulation, ProtocolMatch, modulation_label},
    units::hertz,
};

impl EventPayload for IdentReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
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
            (
                "modulation",
                "Modulation",
                text_of(Modulation::None.label()),
            ),
            (
                "loudest_bin",
                "Loudest bin",
                number(Some(r.snr_db), Unit::Db, 1),
            ),
        ]);
    };
    let mut shown = fields(&[
        ("signals", "Signals", count(Some(r.signals.len() as f64))),
        (
            "frequency",
            "Frequency",
            number(Some(loudest.frequency_hz), Unit::Hz, 0),
        ),
        (
            "modulation",
            "Modulation",
            text_of(modulation_label(loudest)),
        ),
        ("confidence", "Confidence", percent(loudest.confidence)),
    ]);
    shown.extend(signal_fields(loudest));
    shown.extend(fields(&[(
        "candidates",
        "Candidates",
        text(Some(&candidates(r))),
    )]));
    shown
}

fn signal_fields(s: &IdentSignal) -> Vec<Field> {
    let levels = Some(s.features.frequency_levels).filter(|levels| *levels > 1);
    let duty = Some(s.features.duty).filter(|duty| *duty < 0.99);
    fields(&[
        (
            "bandwidth",
            "Bandwidth",
            number(Some(s.bandwidth_hz), Unit::Hz, 0),
        ),
        (
            "off_tune",
            "Off tune",
            number(Some(s.center_offset_hz), Unit::Hz, 0),
        ),
        ("snr", "SNR", number(Some(s.snr_db), Unit::Db, 1)),
        (
            "symbol_rate",
            "Symbol rate",
            number(s.symbol_rate_hz, Unit::Baud, 0),
        ),
        (
            "deviation",
            "Deviation",
            number(s.deviation_hz, Unit::Hz, 0),
        ),
        (
            "burst",
            "Burst",
            number(s.burst_ms.map(|ms| ms / 1_000.0), Unit::Seconds, 5),
        ),
        (
            "burst_period",
            "Burst period",
            number(s.burst_period_ms.map(|ms| ms / 1_000.0), Unit::Seconds, 4),
        ),
        (
            "ofdm_symbol",
            "OFDM symbol",
            number(s.ofdm_symbol_us.map(|us| us / 1e6), Unit::Seconds, 6),
        ),
        (
            "ofdm_guard",
            "OFDM guard",
            number(s.ofdm_guard_us.map(|us| us / 1e6), Unit::Seconds, 6),
        ),
        ("duty", "Duty", duty.and_then(percent)),
        ("frequency_levels", "Frequency levels", count(levels)),
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
