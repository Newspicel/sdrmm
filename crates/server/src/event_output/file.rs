use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Weak,
};

use sdrmm_engine::Engine;
use sdrmm_wire::{
    event_file_name,
    event_output::{CSV_SUFFIX, TEXT_SUFFIX},
    valid_event_file,
};

use super::{Delivery, DeliveryError, csv, text};

const EVENTS_DIR: &str = "events";

/// How an output file lays out its events.
#[derive(Clone, Copy, Debug)]
pub(super) enum Format {
    Csv,
    Text,
}

impl Format {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Csv => CSV_SUFFIX,
            Self::Text => TEXT_SUFFIX,
        }
    }

    const fn header(self) -> &'static str {
        match self {
            Self::Csv => csv::HEADER,
            Self::Text => "",
        }
    }

    fn line(self, delivery: &Delivery) -> String {
        let facts = &delivery.message.facts;
        match self {
            Self::Csv => csv::row(&delivery.node, facts),
            Self::Text => text::line(
                &facts.at,
                &facts.kind,
                facts.freq_hz,
                facts.station.as_deref(),
                &facts.summary,
            ),
        }
    }
}

pub(super) fn events_dir(recordings_dir: &Path) -> PathBuf {
    recordings_dir.join(EVENTS_DIR)
}

pub(super) async fn append(
    engine: &Weak<Engine>,
    format: Format,
    file: &str,
    batch: &[Delivery],
) -> Result<(), DeliveryError> {
    if !valid_event_file(file) {
        return Err(DeliveryError::Failed(format!(
            "File {file:?} is not a plain name"
        )));
    }
    let engine = engine
        .upgrade()
        .ok_or_else(|| DeliveryError::Failed("the engine has stopped".to_owned()))?;
    let path = engine
        .recordings_dir()
        .map(|dir| events_dir(dir).join(event_file_name(file, format.suffix())))
        .ok_or_else(|| DeliveryError::Failed("no recordings directory configured".to_owned()))?;
    let lines: String = batch.iter().map(|delivery| format.line(delivery)).collect();
    let header = format.header();
    tokio::task::spawn_blocking(move || write_lines(&path, header, &lines))
        .await
        .map_err(|error| DeliveryError::Failed(format!("File write stopped: {error}")))?
        .map_err(|error| DeliveryError::Failed(format!("File write failed: {error}")))
}

fn write_lines(path: &Path, header: &str, lines: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut text = String::with_capacity(header.len() + lines.len());
    if file.metadata()?.len() == 0 {
        text.push_str(header);
    }
    text.push_str(lines);
    file.write_all(text.as_bytes())
}
