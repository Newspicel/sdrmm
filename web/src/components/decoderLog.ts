import { decoderLogExportUrl } from "../lib/api";
import type {
  Callsign,
  DecodedRecord,
  DecoderLogEntry,
  DecoderLogFilter,
  EventAudio,
  Field,
  Presentation,
} from "../lib/types";
import type { DownloadChoice } from "./DownloadMenu";

export const LIMIT_OPTIONS = [100, 500, 2000];

export const LIVE_ROW_CAP = 200;

export interface LogFilter {
  q: string;
  limit: number;
}

export const DEFAULT_LOG_FILTER: LogFilter = { q: "", limit: 500 };

export interface LogRow {
  key: string;
  at: string;
  kind: string;
  station: string | null;
  callsign: Callsign | null;
  summary: string;
  freqHz: number;
  source: string;
  live: boolean;
  fields: readonly Field[];
  clip: EventAudio | null;
}

export const LOG_COLUMNS = [
  { key: "at", label: "Time", width: 84 },
  { key: "kind", label: "Kind", width: 96 },
  { key: "freq", label: "Frequency", width: 116 },
  { key: "station", label: "Station", width: 152 },
  { key: "summary", label: "Summary", width: 180 },
] as const;

export type LogColumnKey = (typeof LOG_COLUMNS)[number]["key"];

export const FLEX_COLUMN: LogColumnKey = "summary";

export type ColumnWidths = Record<LogColumnKey, number>;

export const MIN_COLUMN_WIDTH = 56;

export const MAX_COLUMN_WIDTH = 720;

export const COLUMN_STEP = 8;

const COLUMN_KEY = "sdrmm.decoderLog.columns";

export function defaultColumnWidths(): ColumnWidths {
  const widths = {} as ColumnWidths;
  for (const column of LOG_COLUMNS) {
    widths[column.key] = column.width;
  }
  return widths;
}

export function clampColumnWidth(px: number): number {
  if (!Number.isFinite(px)) {
    return MIN_COLUMN_WIDTH;
  }
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, px)));
}

export function resizeColumn(widths: ColumnWidths, key: LogColumnKey, px: number): ColumnWidths {
  return { ...widths, [key]: clampColumnWidth(px) };
}

export function totalColumnWidth(widths: ColumnWidths): number {
  return LOG_COLUMNS.reduce((sum, column) => sum + widths[column.key], 0);
}

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export function readColumnWidths(): ColumnWidths {
  const widths = defaultColumnWidths();
  let stored: unknown;
  try {
    const raw = storage()?.getItem(COLUMN_KEY);
    stored = raw === null || raw === undefined ? null : JSON.parse(raw);
  } catch {
    return widths;
  }
  if (stored === null || typeof stored !== "object") {
    return widths;
  }
  const record = stored as Record<string, unknown>;
  for (const column of LOG_COLUMNS) {
    const value = record[column.key];
    if (column.key !== FLEX_COLUMN && typeof value === "number" && Number.isFinite(value)) {
      widths[column.key] = clampColumnWidth(value);
    }
  }
  return widths;
}

export function writeColumnWidths(widths: ColumnWidths): void {
  try {
    storage()?.setItem(COLUMN_KEY, JSON.stringify(widths));
  } catch {}
}

export interface WireScope {
  sink: string;
  wired: boolean;
}

export function toQuery(filter: LogFilter, wires: WireScope): DecoderLogFilter {
  const query: DecoderLogFilter = {
    limit: filter.limit,
    sink: wires.sink,
  };
  const q = filter.q.trim();
  if (q !== "") {
    query.q = q;
  }
  return query;
}

export function isFiltered(filter: LogFilter): boolean {
  return filter.q.trim() !== "";
}

const NO_VIEW: Presentation = { summary: "" };

export function matchesFilter(record: DecodedRecord, filter: LogFilter): boolean {
  const q = filter.q.trim().toLowerCase();
  if (q === "") {
    return true;
  }
  const view = record.view ?? NO_VIEW;
  return (
    view.summary.toLowerCase().includes(q) ||
    (view.entity != null && view.entity.toLowerCase().includes(q))
  );
}

export function collectLive(
  records: readonly DecodedRecord[],
  filter: LogFilter,
  clearedAt: number,
  cap = LIVE_ROW_CAP,
): DecodedRecord[] {
  const kept = records.filter(
    (record) => timeMs(record.at) > clearedAt && matchesFilter(record, filter),
  );
  kept.sort((a, b) => timeMs(b.at) - timeMs(a.at));
  return kept.length > cap ? kept.slice(0, cap) : kept;
}

export function buildRows(
  entries: readonly DecoderLogEntry[],
  live: readonly DecodedRecord[],
): LogRow[] {
  const stored = entries.map(storedRow);
  const seen = new Set(stored.map(rowSignature));
  const rows: LogRow[] = [];
  for (const record of live) {
    const row = liveRow(record);
    const key = rowSignature(row);
    if (!seen.has(key)) {
      seen.add(key);
      rows.push(row);
    }
  }
  rows.push(...stored);
  rows.sort((a, b) => timeMs(b.at) - timeMs(a.at));
  return rows;
}

function rowOf(
  at: string,
  kind: string,
  view: Presentation,
  freqHz: number,
  source: string,
  callsign: Callsign | null,
): Omit<LogRow, "key" | "live"> {
  return {
    at,
    kind,
    station: view.entity ?? null,
    callsign,
    summary: view.summary,
    freqHz,
    source,
    fields: view.fields ?? [],
    clip: view.clip ?? null,
  };
}

export function storedRow(entry: DecoderLogEntry): LogRow {
  const view = entry.view ?? { summary: entry.summary, entity: entry.station };
  return {
    key: `stored:${entry.id}`,
    live: false,
    ...rowOf(
      entry.at,
      entry.kind,
      view,
      entry.freq_hz,
      entry.origin?.node ?? entry.node ?? "",
      entry.callsign ?? null,
    ),
  };
}

export function liveRow(record: DecodedRecord): LogRow {
  const row: LogRow = {
    key: "",
    live: true,
    ...rowOf(
      record.at,
      record.event.kind,
      record.view ?? NO_VIEW,
      record.freq_hz,
      record.origin?.node ?? "",
      record.callsign ?? null,
    ),
  };
  row.key = `live:${rowSignature(row)}`;
  return row;
}

export function callsignTitle(callsign: Callsign): string {
  return `${callsign.country}, ${callsign.continent}, CQ ${callsign.cq_zone}, ITU ${callsign.itu_zone}`;
}

export function droppedNotice(lost: number, dropped: number): string | null {
  if (lost <= 0 && dropped <= 0) {
    return null;
  }
  const parts: string[] = [];
  if (lost > 0) {
    parts.push(`${lost} live ${frameWord(lost)} dropped`);
  }
  if (dropped > 0) {
    parts.push(`${dropped} ${frameWord(dropped)} never reached the log`);
  }
  return parts.join(" · ");
}

function frameWord(n: number): string {
  return n === 1 ? "frame" : "frames";
}

export function rowSignature(row: LogRow): string {
  return `${row.at}|${row.kind}|${row.source}|${row.station ?? ""}|${row.summary}`;
}

function timeMs(at: string): number {
  const ms = Date.parse(at);
  return Number.isNaN(ms) ? 0 : ms;
}

export function logDownloads(filter: DecoderLogFilter): DownloadChoice[] {
  return [
    { label: "CSV", href: decoderLogExportUrl("csv", filter), hint: "One row per decoded frame" },
    { label: "JSON", href: decoderLogExportUrl("json", filter), hint: "Every field of every row" },
  ];
}
