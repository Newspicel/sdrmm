use rusqlite::{Connection, OptionalExtension, Row, params};
use sdrmm_wire::{
    MAX_NANOVNA_NAME_LEN, MAX_NANOVNA_NOTE_LEN, MAX_NANOVNA_POINTS, NanoVnaAntenna,
    NanoVnaAntennaRequest, NanoVnaComplex, NanoVnaMeasurement, NanoVnaMeasurementInfo,
    NanoVnaPoint, SaveNanoVnaMeasurementRequest, UpdateNanoVnaMeasurementRequest,
};

use super::{Store, StoreError, now_rfc3339};

const MIN_SAVED_POINTS: usize = 2;

const ANTENNA_COLUMNS: &str = "id, name, notes, created_at";
const INFO_COLUMNS: &str = "id, antenna_id, label, note, instrument, created_at, start_hz, \
                            stop_hz, point_count, best_hz, best_vswr";

fn check_name(name: &str, field: &'static str) -> Result<(), StoreError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NANOVNA_NAME_LEN {
        return Err(StoreError::NanoVnaField(field));
    }
    Ok(())
}

fn check_note(note: &str, field: &'static str) -> Result<(), StoreError> {
    if note.chars().count() > MAX_NANOVNA_NOTE_LEN {
        return Err(StoreError::NanoVnaField(field));
    }
    Ok(())
}

fn instrument_of(instrument: Option<&str>) -> Result<Option<&str>, StoreError> {
    let Some(trimmed) = instrument.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    if trimmed.chars().count() > MAX_NANOVNA_NAME_LEN {
        return Err(StoreError::NanoVnaField("instrument"));
    }
    Ok(Some(trimmed))
}

fn finite(value: NanoVnaComplex) -> bool {
    value.re.is_finite() && value.im.is_finite()
}

fn check_points(points: &[NanoVnaPoint]) -> Result<(), StoreError> {
    let most = usize::try_from(MAX_NANOVNA_POINTS).unwrap_or(usize::MAX);
    if !(MIN_SAVED_POINTS..=most).contains(&points.len()) {
        return Err(StoreError::NanoVnaField("point count"));
    }
    if !points
        .iter()
        .all(|point| finite(point.s11) && finite(point.s21))
    {
        return Err(StoreError::NanoVnaField("point values"));
    }
    if !points
        .windows(2)
        .all(|pair| pair[0].frequency_hz < pair[1].frequency_hz)
    {
        return Err(StoreError::NanoVnaField("frequency order"));
    }
    Ok(())
}

fn vswr(point: &NanoVnaPoint) -> Option<f64> {
    let magnitude = point.s11.re.hypot(point.s11.im);
    (magnitude < 1.0).then(|| (1.0 + magnitude) / (1.0 - magnitude))
}

fn best_match(points: &[NanoVnaPoint]) -> Option<(u64, f64)> {
    points
        .iter()
        .filter_map(|point| vswr(point).map(|ratio| (point.frequency_hz, ratio)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn sql_hz(hz: u64) -> Result<i64, StoreError> {
    i64::try_from(hz).map_err(|_| StoreError::NanoVnaField("frequency"))
}

fn hz_at(row: &Row<'_>, index: usize) -> Result<u64, rusqlite::Error> {
    let raw: i64 = row.get(index)?;
    u64::try_from(raw).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, raw))
}

fn optional_hz_at(row: &Row<'_>, index: usize) -> Result<Option<u64>, rusqlite::Error> {
    row.get::<_, Option<i64>>(index)?
        .map(|raw| {
            u64::try_from(raw).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, raw))
        })
        .transpose()
}

fn antenna_from(row: &Row<'_>) -> Result<NanoVnaAntenna, rusqlite::Error> {
    Ok(NanoVnaAntenna {
        id: row.get(0)?,
        name: row.get(1)?,
        notes: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn info_from(row: &Row<'_>) -> Result<NanoVnaMeasurementInfo, rusqlite::Error> {
    Ok(NanoVnaMeasurementInfo {
        id: row.get(0)?,
        antenna_id: row.get(1)?,
        label: row.get(2)?,
        note: row.get(3)?,
        instrument: row.get(4)?,
        created_at: row.get(5)?,
        start_hz: hz_at(row, 6)?,
        stop_hz: hz_at(row, 7)?,
        point_count: row.get(8)?,
        best_hz: optional_hz_at(row, 9)?,
        best_vswr: row.get(10)?,
    })
}

fn read_antenna(conn: &Connection, id: i64) -> Result<NanoVnaAntenna, StoreError> {
    conn.query_row(
        &format!("SELECT {ANTENNA_COLUMNS} FROM nanovna_antennas WHERE id = ?1"),
        params![id],
        antenna_from,
    )
    .optional()?
    .ok_or(StoreError::NanoVnaAntennaNotFound(id))
}

fn read_info(conn: &Connection, id: i64) -> Result<NanoVnaMeasurementInfo, StoreError> {
    conn.query_row(
        &format!("SELECT {INFO_COLUMNS} FROM nanovna_measurements WHERE id = ?1"),
        params![id],
        info_from,
    )
    .optional()?
    .ok_or(StoreError::NanoVnaMeasurementNotFound(id))
}

fn insert_measurement(
    conn: &Connection,
    antenna_id: i64,
    request: &SaveNanoVnaMeasurementRequest,
    instrument: Option<&str>,
) -> Result<i64, StoreError> {
    let (first, last) = match (request.points.first(), request.points.last()) {
        (Some(first), Some(last)) => (first.frequency_hz, last.frequency_hz),
        _ => return Err(StoreError::NanoVnaField("point count")),
    };
    let best = best_match(&request.points);
    let best_hz = best.map(|(hz, _)| sql_hz(hz)).transpose()?;
    let point_count =
        u32::try_from(request.points.len()).map_err(|_| StoreError::NanoVnaField("point count"))?;
    conn.execute(
        "INSERT INTO nanovna_measurements (antenna_id, label, note, instrument, created_at, \
         start_hz, stop_hz, point_count, best_hz, best_vswr, points) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            antenna_id,
            request.label.trim(),
            request.note,
            instrument,
            now_rfc3339(),
            sql_hz(first)?,
            sql_hz(last)?,
            point_count,
            best_hz,
            best.map(|(_, ratio)| ratio),
            serde_json::to_string(&request.points)?
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

impl Store {
    pub fn create_nanovna_antenna(
        &self,
        request: &NanoVnaAntennaRequest,
    ) -> Result<NanoVnaAntenna, StoreError> {
        check_name(&request.name, "name")?;
        check_note(&request.notes, "notes")?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO nanovna_antennas (name, notes, created_at) VALUES (?1, ?2, ?3)",
            params![request.name.trim(), request.notes, now_rfc3339()],
        )
        .map_err(taken_name)?;
        read_antenna(&conn, conn.last_insert_rowid())
    }

    pub fn update_nanovna_antenna(
        &self,
        id: i64,
        request: &NanoVnaAntennaRequest,
    ) -> Result<NanoVnaAntenna, StoreError> {
        check_name(&request.name, "name")?;
        check_note(&request.notes, "notes")?;
        let conn = self.lock();
        let changed = conn
            .execute(
                "UPDATE nanovna_antennas SET name = ?2, notes = ?3 WHERE id = ?1",
                params![id, request.name.trim(), request.notes],
            )
            .map_err(taken_name)?;
        if changed == 0 {
            return Err(StoreError::NanoVnaAntennaNotFound(id));
        }
        read_antenna(&conn, id)
    }

    pub fn delete_nanovna_antenna(&self, id: i64) -> Result<(), StoreError> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM nanovna_measurements WHERE antenna_id = ?1",
            params![id],
        )?;
        let changed = tx.execute("DELETE FROM nanovna_antennas WHERE id = ?1", params![id])?;
        if changed == 0 {
            return Err(StoreError::NanoVnaAntennaNotFound(id));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_nanovna_antennas(&self) -> Result<Vec<NanoVnaAntenna>, StoreError> {
        let conn = self.lock();
        let mut statement = conn.prepare(&format!(
            "SELECT {ANTENNA_COLUMNS} FROM nanovna_antennas ORDER BY name COLLATE NOCASE, id"
        ))?;
        let antennas = statement
            .query_map([], antenna_from)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(antennas)
    }

    pub fn nanovna_antenna(&self, id: i64) -> Result<NanoVnaAntenna, StoreError> {
        read_antenna(&self.lock(), id)
    }

    pub fn save_nanovna_measurement(
        &self,
        antenna_id: i64,
        request: &SaveNanoVnaMeasurementRequest,
    ) -> Result<NanoVnaMeasurementInfo, StoreError> {
        check_name(&request.label, "label")?;
        check_note(&request.note, "note")?;
        let instrument = instrument_of(request.instrument.as_deref())?;
        check_points(&request.points)?;
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        read_antenna(&tx, antenna_id)?;
        let id = insert_measurement(&tx, antenna_id, request, instrument)?;
        let info = read_info(&tx, id)?;
        tx.commit()?;
        Ok(info)
    }

    pub fn update_nanovna_measurement(
        &self,
        id: i64,
        request: &UpdateNanoVnaMeasurementRequest,
    ) -> Result<NanoVnaMeasurementInfo, StoreError> {
        check_name(&request.label, "label")?;
        check_note(&request.note, "note")?;
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE nanovna_measurements SET label = ?2, note = ?3 WHERE id = ?1",
            params![id, request.label.trim(), request.note],
        )?;
        if changed == 0 {
            return Err(StoreError::NanoVnaMeasurementNotFound(id));
        }
        read_info(&conn, id)
    }

    pub fn delete_nanovna_measurement(&self, id: i64) -> Result<(), StoreError> {
        let conn = self.lock();
        let changed = conn.execute(
            "DELETE FROM nanovna_measurements WHERE id = ?1",
            params![id],
        )?;
        (changed > 0)
            .then_some(())
            .ok_or(StoreError::NanoVnaMeasurementNotFound(id))
    }

    pub fn list_nanovna_measurements(&self) -> Result<Vec<NanoVnaMeasurementInfo>, StoreError> {
        let conn = self.lock();
        let mut statement = conn.prepare(&format!(
            "SELECT {INFO_COLUMNS} FROM nanovna_measurements ORDER BY created_at DESC, id DESC"
        ))?;
        let measurements = statement
            .query_map([], info_from)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(measurements)
    }

    pub fn nanovna_measurement(&self, id: i64) -> Result<NanoVnaMeasurement, StoreError> {
        let conn = self.lock();
        let info = read_info(&conn, id)?;
        let body: String = conn.query_row(
            "SELECT points FROM nanovna_measurements WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(NanoVnaMeasurement {
            info,
            points: serde_json::from_str(&body)?,
        })
    }
}

fn taken_name(error: rusqlite::Error) -> StoreError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            StoreError::NanoVnaNameTaken
        }
        _ => StoreError::Db(error),
    }
}

#[cfg(test)]
mod tests;
