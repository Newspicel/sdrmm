use std::collections::HashMap;

use rusqlite::{Connection, params_from_iter, types::Value};
use sdrmm_wire::{DecoderLogEntry, DecoderLogGroup, DecoderLogQuery, LogGroupKey};

use super::{
    DECODER_LOG_LIMIT_DEFAULT, DECODER_LOG_LIMIT_MAX, LOG_COLUMNS, Store, StoreError,
    read_log_entries,
};

const AIRTIME: &str = "SUM(CASE WHEN decoder_log.kind IN ('call', 'transmission') \
     THEN json_extract(decoder_log.event, '$.data.duration_ms') END)";

fn key_of(by: LogGroupKey) -> &'static str {
    match by {
        LogGroupKey::Frequency => "CAST(ROUND(decoder_log.freq_hz / 100.0) AS INTEGER)",
        LogGroupKey::Station => "decoder_log.kind || char(31) || COALESCE(decoder_log.station, '')",
    }
}

struct Tally {
    count: i64,
    first_at: String,
    last_at: String,
    latest: i64,
    airtime_ms: Option<i64>,
}

impl Store {
    pub fn group_decoder_log(
        &self,
        filter: &DecoderLogQuery,
        by: LogGroupKey,
    ) -> Result<(Vec<DecoderLogGroup>, u64), StoreError> {
        let limit = filter
            .limit
            .unwrap_or(DECODER_LOG_LIMIT_DEFAULT)
            .min(DECODER_LOG_LIMIT_MAX);
        let predicate = self.decoder_log_predicate(filter)?;
        let key = key_of(by);
        let conn = self.lock();
        let total: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(DISTINCT {key}) FROM {}{}",
                predicate.from, predicate.clause
            ),
            params_from_iter(&predicate.params),
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "SELECT COUNT(*), MIN(decoder_log.at), MAX(decoder_log.at), MAX(decoder_log.id), \
             {AIRTIME} FROM {}{} GROUP BY {key} ORDER BY MAX(decoder_log.at) DESC LIMIT ?",
            predicate.from, predicate.clause
        ))?;
        let mut params = predicate.params.clone();
        params.push(Value::Integer(i64::from(limit)));
        let tallies = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(Tally {
                    count: row.get(0)?,
                    first_at: row.get(1)?,
                    last_at: row.get(2)?,
                    latest: row.get(3)?,
                    airtime_ms: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut latest = entries_by_id(&conn, tallies.iter().map(|tally| tally.latest))?;
        let groups = tallies
            .into_iter()
            .map(|tally| {
                let entry = latest
                    .remove(&tally.latest)
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                Ok(DecoderLogGroup {
                    count: tally.count.max(0).unsigned_abs(),
                    first_at: tally.first_at,
                    last_at: tally.last_at,
                    airtime_ms: tally.airtime_ms.map(|ms| ms.max(0).unsigned_abs()),
                    latest: entry,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok((groups, total.max(0).unsigned_abs()))
    }
}

fn entries_by_id(
    conn: &Connection,
    ids: impl Iterator<Item = i64>,
) -> Result<HashMap<i64, DecoderLogEntry>, StoreError> {
    let params: Vec<Value> = ids.map(Value::Integer).collect();
    if params.is_empty() {
        return Ok(HashMap::new());
    }
    let holes = vec!["?"; params.len()].join(", ");
    let sql = format!("SELECT {LOG_COLUMNS} FROM decoder_log WHERE decoder_log.id IN ({holes})");
    Ok(read_log_entries(conn, &sql, params)?
        .into_iter()
        .map(|entry| (entry.id, entry))
        .collect())
}

#[cfg(test)]
mod tests;
