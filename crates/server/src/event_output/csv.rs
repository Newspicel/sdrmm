use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Weak,
};

use sdrmm_engine::Engine;
use sdrmm_wire::{csv_file_name, valid_csv_file};

use super::{Delivery, DeliveryError, EventFacts};

const EVENTS_DIR: &str = "events";
const HEADER: &str = "at,kind,output,device_set,channel,freq_hz,station,summary,data\r\n";
const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

pub(super) fn events_dir(recordings_dir: &Path) -> PathBuf {
    recordings_dir.join(EVENTS_DIR)
}

pub(super) async fn append(
    engine: &Weak<Engine>,
    file: &str,
    batch: &[Delivery],
) -> Result<(), DeliveryError> {
    if !valid_csv_file(file) {
        return Err(DeliveryError::Failed(format!(
            "CSV file {file:?} is not a plain name"
        )));
    }
    let engine = engine
        .upgrade()
        .ok_or_else(|| DeliveryError::Failed("the engine has stopped".to_owned()))?;
    let path = engine
        .recordings_dir()
        .map(|dir| events_dir(dir).join(csv_file_name(file)))
        .ok_or_else(|| DeliveryError::Failed("no recordings directory configured".to_owned()))?;
    let rows: String = batch
        .iter()
        .map(|delivery| row(&delivery.node, &delivery.message.facts))
        .collect();
    tokio::task::spawn_blocking(move || write_rows(&path, &rows))
        .await
        .map_err(|error| DeliveryError::Failed(format!("CSV write stopped: {error}")))?
        .map_err(|error| DeliveryError::Failed(format!("CSV write failed: {error}")))
}

fn write_rows(path: &Path, rows: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut text = String::with_capacity(HEADER.len() + rows.len());
    if file.metadata()?.len() == 0 {
        text.push_str(HEADER);
    }
    text.push_str(rows);
    file.write_all(text.as_bytes())
}

pub(super) fn row(output_node: &str, facts: &EventFacts) -> String {
    let data = facts
        .record
        .pointer("/event/data")
        .map(serde_json::Value::to_string)
        .unwrap_or_default();
    let fields = [
        field(&facts.at),
        field(facts.kind),
        field(output_node),
        facts.device_set.to_string(),
        facts.channel.to_string(),
        facts.freq_hz.to_string(),
        field(facts.station.as_deref().unwrap_or_default()),
        field(&facts.summary),
        field(&data),
    ];
    let mut line = fields.join(",");
    line.push_str("\r\n");
    line
}

fn field(value: &str) -> String {
    let guarded = if value.starts_with(FORMULA_STARTS) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}
