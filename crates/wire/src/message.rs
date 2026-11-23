//! A message layout an operator writes for an output, filled per event.

#[cfg(test)]
mod tests;

pub const MAX_MESSAGE_LEN: usize = 2_000;

/// Names every event fills. `record.` followed by a path reaches the decoder's own record,
/// and any other name a field the decoder presents.
pub const MESSAGE_PLACEHOLDERS: &[&str] = &[
    "headline", "summary", "station", "freq", "time", "kind", "channel", "record.",
];

#[must_use]
pub fn valid_message(layout: &str) -> bool {
    layout.chars().count() <= MAX_MESSAGE_LEN
}

/// Writes `layout` with each `{name}` replaced by what `lookup` gives for it. A name it
/// does not know leaves nothing, `{{` and `}}` write a brace, and a `{` with no closing
/// brace on its line stays as written.
#[must_use]
pub fn fill(layout: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(layout.len());
    let mut rest = layout;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix("{{").or_else(|| tail.strip_prefix("}}")) {
            out.push_str(&tail[..1]);
            rest = after;
            continue;
        }
        if let Some((name, after)) = placeholder(tail) {
            out.push_str(&lookup(name.trim()).unwrap_or_default());
            rest = after;
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn placeholder(tail: &str) -> Option<(&str, &str)> {
    let inner = tail.strip_prefix('{')?;
    let end = inner.find(['}', '{', '\n'])?;
    inner[end..]
        .starts_with('}')
        .then(|| (&inner[..end], &inner[end + 1..]))
}
