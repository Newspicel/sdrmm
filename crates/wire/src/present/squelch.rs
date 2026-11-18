use super::{EventPayload, Presentation, Unit, fields, joined, number, text_of};
use crate::decode::{EventKind, ToneSquelchStatus};

impl EventPayload for ToneSquelchStatus {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let mut parts = Vec::new();
        if let Some(hz) = self.ctcss_hz {
            parts.push(format!("CTCSS {hz:.1} Hz"));
        }
        if let Some(code) = self.dcs_code {
            parts.push(format!("DCS {code:03}"));
        }
        if parts.is_empty() {
            parts.push("no tone".to_owned());
        }
        let squelch = if self.open { "open" } else { "muted" };
        parts.push(squelch.to_owned());
        Presentation {
            fields: fields(&[
                ("ctcss", "CTCSS", number(self.ctcss_hz, Unit::Hz, 1)),
                (
                    "dcs",
                    "DCS",
                    self.dcs_code.and_then(|code| text_of(format!("{code:03}"))),
                ),
                ("audio", "Audio", text_of(squelch)),
            ]),
            verified: Some(false),
            ..Presentation::of(joined(parts))
        }
    }
}
