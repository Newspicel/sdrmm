use super::{
    EventPayload, Field, FieldValue, Presentation, Unit, count, fields, flag, joined, number,
    text_of,
};
use crate::decode::{EventKind, SstvPicture};

impl EventPayload for SstvPicture {
    fn present(&self, _kind: EventKind) -> Presentation {
        let outcome = if self.complete {
            format!("complete in {} s", self.duration_ms / 1_000)
        } else {
            format!("{} of {} lines", self.lines, self.height)
        };
        Presentation {
            fields: picture_fields(
                &[
                    ("Mode", text_of(self.mode.label())),
                    ("Size", picture_size(self.width, self.height)),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(joined([
                self.mode.label().to_owned(),
                format!("{}×{}", self.width, self.height),
                outcome,
            ]))
        }
    }
}

pub(crate) fn picture_size(width: u16, height: u16) -> Option<FieldValue> {
    text_of(format!("{width} × {height}"))
}

fn took(duration_ms: u32) -> Option<FieldValue> {
    number(Some(f64::from(duration_ms) / 1_000.0), Unit::Seconds, 1)
}

pub(crate) fn picture_fields(
    lead: &[(&str, Option<FieldValue>)],
    lines: u16,
    complete: bool,
    duration_ms: u32,
) -> Vec<Field> {
    let mut shown = fields(lead);
    shown.extend(fields(&[
        ("Lines", count(Some(lines))),
        ("Complete", flag(Some(complete))),
        ("Took", took(duration_ms)),
    ]));
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::SstvMode;

    #[test]
    fn sstv_shows_mode_size_and_progress() {
        let view = SstvPicture {
            seq: 1,
            mode: SstvMode::MartinM1,
            width: 320,
            height: 256,
            lines: 200,
            complete: false,
            duration_ms: 90_500,
        }
        .present(EventKind::Sstv);
        assert_eq!(view.labels(), ["Mode", "Size", "Lines", "Complete", "Took"]);
        assert_eq!(view.field("Mode"), text_of("Martin M1").as_ref());
        assert_eq!(view.field("Size"), text_of("320 × 256").as_ref());
        assert_eq!(view.field("Complete"), flag(Some(false)).as_ref());
        assert_eq!(
            view.field("Took"),
            number(Some(90.5), Unit::Seconds, 1).as_ref()
        );
        assert_eq!(view.entity, None);
    }
}
