import type { SidReport, SidStationLevel } from "./types";

export function sidDeviation(station: SidStationLevel): number | null {
  return station.baseline_dbfs == null ? null : station.level_dbfs - station.baseline_dbfs;
}

export function wholeDb(value: number): string {
  return Math.round(value) === 0 ? "0" : value.toFixed(0);
}

export function berText(ber: number): string {
  return ber === 0 ? "0" : ber.toExponential(1);
}

export function signedDb(value: number): string {
  return `${value >= 0 ? "+" : ""}${value.toFixed(1)} dB`;
}

export function sidTrends(
  reports: readonly { at: string; report: SidReport }[],
): Map<string, number[]> {
  const trends = new Map<string, number[]>();
  const ordered = reports.toSorted((a, b) => Date.parse(a.at) - Date.parse(b.at));
  for (const { report } of ordered) {
    for (const station of report.stations) {
      const key = `${station.call}:${station.frequency_hz}`;
      trends.set(key, [...(trends.get(key) ?? []), station.level_dbfs]);
    }
  }
  return trends;
}

export function sparkPoints(values: readonly number[], width: number, height: number): string {
  if (values.length < 2) {
    return "";
  }
  const low = Math.min(...values);
  const span = Math.max(...values) - low || 1;
  const step = width / (values.length - 1);
  return values
    .map(
      (value, k) =>
        `${(k * step).toFixed(1)},${(height - ((value - low) / span) * height).toFixed(1)}`,
    )
    .join(" ");
}
