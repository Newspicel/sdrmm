import { type EntityOf, lastSeenMs } from "../lib/sinks";
import type { AdsbMessage, AisMessage, DecodedRecordOf } from "../lib/types";

export function latestRecord<R extends { at: string }>(records: readonly R[]): R | null {
  return records.reduce<R | null>(
    (latest, record) =>
      latest === null || Date.parse(record.at) > Date.parse(latest.at) ? record : latest,
    null,
  );
}

export function chronological<R extends { at: string }>(records: readonly R[]): R[] {
  return records.toReversed().toSorted((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

export function highest(a: number | null, b: number | null): number | null {
  return a === null ? b : b === null ? a : Math.max(a, b);
}

export function lowest(a: number | null, b: number | null): number | null {
  return a === null ? b : b === null ? a : Math.min(a, b);
}

export const TARGET_STALE_MS = 30_000;

export const TARGET_MAX_AGE_MS = 300_000;

export function ageClass(ageMs: number): string {
  if (ageMs < TARGET_STALE_MS) {
    return "text-ink";
  }
  return ageMs < TARGET_MAX_AGE_MS / 2 ? "text-ink-dim" : "text-ink-dim opacity-45";
}

export function formatAge(ageMs: number): string {
  const seconds = Math.max(0, Math.round(ageMs / 1000));
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const m = Math.floor(seconds / 60);
  return m < 60 ? `${m}:${pad2(seconds % 60)}` : `${Math.floor(m / 60)}h${pad2(m % 60)}`;
}

export interface TargetRow {
  id: string;
  label: string;
  primary: string;
  secondary: string;
  position: string;
  ageMs: number;
  frames: number;
}

export function aircraftRow(entity: EntityOf<"adsb">, nowMs: number): TargetRow {
  const m: AdsbMessage = entity.event.data;
  return {
    id: m.icao.toUpperCase(),
    label: m.callsign?.trim() || "-",
    primary: m.on_ground === true ? "GND" : formatAltitudeFt(m.altitude_ft),
    secondary: joinFields(formatSpeedKt(m.ground_speed_kt), formatBearing(m.track_deg)),
    position: formatPosition(m.lat, m.lon),
    ageMs: Math.max(0, nowMs - lastSeenMs(entity)),
    frames: entity.frames,
  };
}

export function shipRow(entity: EntityOf<"ais">, nowMs: number): TargetRow {
  const m: AisMessage = entity.event.data;
  return {
    id: String(m.mmsi),
    label: m.name?.trim() || m.call_sign?.trim() || "-",
    primary: formatSpeedKt(m.sog_kt),
    secondary: joinFields(formatBearing(m.cog_deg), m.destination?.trim() ?? ""),
    position: formatPosition(m.lat, m.lon),
    ageMs: Math.max(0, nowMs - lastSeenMs(entity)),
    frames: entity.frames,
  };
}

export function targetRow(entity: EntityOf<"adsb"> | EntityOf<"ais">, nowMs: number): TargetRow {
  return entity.event.kind === "adsb"
    ? aircraftRow(entity as EntityOf<"adsb">, nowMs)
    : shipRow(entity as EntityOf<"ais">, nowMs);
}

export type TargetSort = "age" | "id";

export function sortTargets(
  rows: readonly TargetRow[],
  key: TargetSort,
  descending: boolean,
): TargetRow[] {
  const direction = descending ? -1 : 1;
  return rows.toSorted(
    (a, b) => direction * (key === "age" ? a.ageMs - b.ageMs : compareIds(a.id, b.id)),
  );
}

export function formatAltitudeFt(ft: number | null | undefined): string {
  return ft == null ? "-" : `${groupThousands(Math.round(ft))} ft`;
}

export function formatSpeedKt(kt: number | null | undefined): string {
  return kt == null ? "-" : `${kt.toFixed(0)} kt`;
}

export function formatBearing(deg: number | null | undefined): string {
  if (deg == null) {
    return "";
  }
  return `${((Math.round(deg) % 360) + 360) % 360}°`;
}

export function formatPosition(
  lat: number | null | undefined,
  lon: number | null | undefined,
): string {
  return lat == null || lon == null ? "-" : `${lat.toFixed(5)}, ${lon.toFixed(5)}`;
}

export function formatClock(at: string): string {
  const t = new Date(at);
  if (Number.isNaN(t.getTime())) {
    return "--:--:--";
  }
  return `${pad2(t.getHours())}:${pad2(t.getMinutes())}:${pad2(t.getSeconds())}`;
}

export const TRANSCRIPT_LIMIT = 20_000;

export function appendTranscript(
  previous: string,
  chunk: string,
  limit = TRANSCRIPT_LIMIT,
): string {
  const joined = previous + chunk;
  if (joined.length <= limit) {
    return joined;
  }
  const cut = joined.slice(joined.length - limit);
  const newline = cut.indexOf("\n");
  return newline === -1 ? cut : cut.slice(newline + 1);
}

export function buildTranscript(
  records: readonly DecodedRecordOf<"rtty" | "morse" | "psk">[],
  limit = TRANSCRIPT_LIMIT,
): string {
  return records.reduceRight((text, r) => appendTranscript(text, r.event.data.text, limit), "");
}

export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

export function isAtBottom(m: ScrollMetrics, tolerancePx = 8): boolean {
  return m.scrollHeight - m.scrollTop - m.clientHeight <= tolerancePx;
}

function compareIds(a: string, b: string): number {
  if (a.length !== b.length) {
    return a.length - b.length;
  }
  return a < b ? -1 : a > b ? 1 : 0;
}

export function joinFields(...fields: string[]): string {
  return fields.filter((f) => f !== "").join(" · ");
}

function groupThousands(n: number): string {
  const sign = n < 0 ? "−" : "";
  return sign + String(Math.abs(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

export function latestWpm(records: readonly DecodedRecordOf<"morse">[]): number | null {
  const newest = records[0];
  return newest === undefined ? null : newest.event.data.wpm;
}
