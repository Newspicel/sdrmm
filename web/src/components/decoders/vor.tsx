import {
  ChipField,
  limitOf,
  type Mode,
  NumberChip,
  OptionalNumberChip,
  SettingChip,
  TextAutocomplete,
} from "./kit";

export function VorChips({ params, limits, onParams }: Mode<"vor">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "vor", settings: next });
  const station = settings.station ?? "";
  return (
    <>
      <SettingChip
        label="Station"
        value={station === "" ? "none" : station}
        quiet={station === ""}
        title="VOR station identifier"
      >
        {() => (
          <ChipField label="VOR station identifier">
            <TextAutocomplete
              className="min-w-0 flex-1"
              label="VOR station identifier"
              value={station}
              suggestions={[]}
              placeholder="Optional identifier"
              onCommit={(next) => {
                set({ ...settings, station: next === "" ? null : next });
                return true;
              }}
            />
          </ChipField>
        )}
      </SettingChip>
      <OptionalNumberChip
        label="Lat"
        title="VOR station latitude"
        placeholder="Unknown"
        value={settings.station_lat ?? null}
        {...limitOf(limits, "station_lat")}
        unit="°"
        onCommit={(station_lat) => set({ ...settings, station_lat })}
      />
      <OptionalNumberChip
        label="Lon"
        title="VOR station longitude"
        placeholder="Unknown"
        value={settings.station_lon ?? null}
        {...limitOf(limits, "station_lon")}
        unit="°"
        onCommit={(station_lon) => set({ ...settings, station_lon })}
      />
      <NumberChip
        label="Declination"
        title="East-positive magnetic declination at the VOR"
        value={settings.magnetic_declination_deg}
        {...limitOf(limits, "magnetic_declination_deg")}
        unit="°"
        onCommit={(magnetic_declination_deg) => set({ ...settings, magnetic_declination_deg })}
      />
      <NumberChip
        label="Every"
        title="VOR report interval"
        value={settings.report_ms}
        {...limitOf(limits, "report_ms")}
        unit="ms"
        onCommit={(report_ms) => set({ ...settings, report_ms })}
      />
    </>
  );
}

import { EMPTY, PANE, type PanelProps, TABLE_CELL, TABLE_HEAD } from "./panelKit";

export function VorView({ records }: PanelProps<"vor">) {
  const readings = latestVorReadings(records);
  const fix = multiVorFix(readings);
  if (readings.length === 0) {
    return (
      <div className={PANE}>
        <span className={EMPTY}>No VOR reports yet.</span>
      </div>
    );
  }
  return (
    <div className={PANE}>
      {fix === null ? (
        <span className={EMPTY}>
          Add coordinates to two non-parallel VOR channels to calculate a position fix.
        </span>
      ) : (
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <span className="font-mono text-xl tabular-nums text-ink">
            {fix.lat.toFixed(5)}, {fix.lon.toFixed(5)}
          </span>
          <span className="text-xs text-ink-dim">
            {fix.stations} stations ·{" "}
            {fix.residualKm < 1
              ? `${Math.round(fix.residualKm * 1000)} m`
              : `${fix.residualKm.toFixed(1)} km`}{" "}
            residual
          </span>
        </div>
      )}
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr>
              <th className={TABLE_HEAD}>Station</th>
              <th className={TABLE_HEAD}>Radial</th>
              <th className={TABLE_HEAD}>Confidence</th>
              <th className={TABLE_HEAD}>Signal</th>
            </tr>
          </thead>
          <tbody>
            {readings.map((record) => {
              const reading = record.event.data;
              return (
                <tr key={reading.station ?? record.origin?.node ?? ""}>
                  <td className={TABLE_CELL}>{reading.station ?? "-"}</td>
                  <td className={`${TABLE_CELL} font-mono tabular-nums`}>
                    {reading.radial_deg.toFixed(1)}°
                  </td>
                  <td className={`${TABLE_CELL} font-mono tabular-nums`}>
                    {Math.round(reading.confidence * 100)}%
                  </td>
                  <td className={`${TABLE_CELL} font-mono tabular-nums`}>
                    {reading.signal_db.toFixed(1)} dB
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

import { type DecodedRecordOf, type VorReading } from "../../lib/types";

export function latestVorReadings(
  records: readonly DecodedRecordOf<"vor">[],
): DecodedRecordOf<"vor">[] {
  const latest = new Map<string, DecodedRecordOf<"vor">>();
  for (const record of records) {
    const key = record.event.data.station ?? record.origin?.node ?? "";
    const previous = latest.get(key);
    if (previous === undefined || Date.parse(record.at) > Date.parse(previous.at)) {
      latest.set(key, record);
    }
  }
  return [...latest.values()].toSorted((a, b) => a.event.data.radial_deg - b.event.data.radial_deg);
}

export interface VorFix {
  lat: number;
  lon: number;
  residualKm: number;
  stations: number;
}

export function multiVorFix(records: readonly DecodedRecordOf<"vor">[]): VorFix | null {
  const usable = records.filter(hasVorPosition);
  if (usable.length < 2) {
    return null;
  }
  const earthRadiusKm = 6371;
  const refLat = average(usable.map((record) => radians(record.event.data.station_lat!)));
  const refLon = averageAngle(usable.map((record) => radians(record.event.data.station_lon!)));
  let a00 = 0;
  let a01 = 0;
  let a11 = 0;
  let b0 = 0;
  let b1 = 0;
  const lines = usable.map((record) => {
    const reading = record.event.data;
    const x =
      earthRadiusKm * normalizeRadians(radians(reading.station_lon!) - refLon) * Math.cos(refLat);
    const y = earthRadiusKm * (radians(reading.station_lat!) - refLat);
    const bearing = radians(reading.radial_deg + reading.magnetic_declination_deg);
    const nx = Math.cos(bearing);
    const ny = -Math.sin(bearing);
    const projection = nx * x + ny * y;
    const weight = Math.max(0.05, reading.confidence) ** 2;
    a00 += weight * nx * nx;
    a01 += weight * nx * ny;
    a11 += weight * ny * ny;
    b0 += weight * nx * projection;
    b1 += weight * ny * projection;
    return { nx, ny, projection, weight };
  });
  const determinant = a00 * a11 - a01 * a01;
  if (Math.abs(determinant) < 1e-8) {
    return null;
  }
  const x = (b0 * a11 - b1 * a01) / determinant;
  const y = (a00 * b1 - a01 * b0) / determinant;
  const weightedError = lines.reduce((sum, line) => {
    const error = line.nx * x + line.ny * y - line.projection;
    return sum + line.weight * error * error;
  }, 0);
  const weight = lines.reduce((sum, line) => sum + line.weight, 0);
  return {
    lat: degreesFromRadians(refLat + y / earthRadiusKm),
    lon: degreesFromRadians(normalizeRadians(refLon + x / (earthRadiusKm * Math.cos(refLat)))),
    residualKm: Math.sqrt(weightedError / weight),
    stations: usable.length,
  };
}

export function hasVorPosition(
  record: DecodedRecordOf<"vor">,
): record is DecodedRecordOf<"vor"> & { event: { data: VorReading } } {
  const { station_lat: lat, station_lon: lon } = record.event.data;
  return lat != null && lon != null && Number.isFinite(lat) && Number.isFinite(lon);
}

export function radians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

export function degreesFromRadians(angle: number): number {
  return (angle * 180) / Math.PI;
}

export function average(values: readonly number[]): number {
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

export function averageAngle(values: readonly number[]): number {
  return Math.atan2(
    values.reduce((sum, value) => sum + Math.sin(value), 0),
    values.reduce((sum, value) => sum + Math.cos(value), 0),
  );
}

export function normalizeRadians(angle: number): number {
  return Math.atan2(Math.sin(angle), Math.cos(angle));
}
