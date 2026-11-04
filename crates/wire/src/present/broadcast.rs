use super::{EventPayload, Presentation, PresentedFile, count, fields, text};
use crate::decode::{BroadcastData, EventKind};

const PLAIN_TEXT: &str = "text/plain";

impl EventPayload for BroadcastData {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let body = (self.media_type == PLAIN_TEXT).then(|| String::from_utf8_lossy(&self.bytes));
        Presentation {
            fields: fields(&[
                ("name", "Name", text(Some(&self.name))),
                ("type", "Type", text(Some(&self.media_type))),
                ("bytes", "Bytes", count(Some(self.bytes.len() as f64))),
                ("text", "Text", text(body.as_deref())),
            ]),
            file: Some(PresentedFile::inline(
                &self.name,
                &self.media_type,
                &self.bytes,
            )),
            ..Presentation::of(format!("{} · {} bytes", self.name, self.bytes.len()))
        }
    }
}
