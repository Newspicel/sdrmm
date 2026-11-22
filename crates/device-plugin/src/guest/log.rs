use std::fmt::Write as _;

use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
    level_filters::LevelFilter,
    span::{Attributes, Id, Record},
};

use crate::abi::{HostV1, Str};

/// A logger that hands every event to the host, so a plugin's `tracing` lines land in the
/// host's log. A plugin library holds its own copy of `tracing`, so it installs this as its
/// global default.
pub struct Forwarder {
    host: &'static HostV1,
}

impl Forwarder {
    #[must_use]
    pub fn new(host: &'static HostV1) -> Self {
        Self { host }
    }
}

pub(super) fn install(host: &'static HostV1) {
    if tracing::subscriber::set_global_default(Forwarder::new(host)).is_err() {
        (host.log)(
            2,
            Str::of(b"sdrmm_device_plugin"),
            Str::of(b"plugin logs stay local: a logger was already installed"),
        );
    }
}

pub(crate) const fn level_code(level: Level) -> u32 {
    match level {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    }
}

struct Text(String);

impl Visit for Text {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.insert_str(0, value);
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.insert_str(0, &format!("{value:?}"));
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

impl Subscriber for Forwarder {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        level_code(*metadata.level()) <= self.host.max_level
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(match self.host.max_level {
            0 => LevelFilter::OFF,
            1 => LevelFilter::ERROR,
            2 => LevelFilter::WARN,
            3 => LevelFilter::INFO,
            4 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        })
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut text = Text(String::new());
        event.record(&mut text);
        let metadata = event.metadata();
        (self.host.log)(
            level_code(*metadata.level()),
            Str::of(metadata.target().as_bytes()),
            Str::of(text.0.as_bytes()),
        );
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}
