import type { DecoderLogGroup, LogGroupKey } from "../lib/types";
import { type LogRow, storedRow } from "./decoderLog";
import { formatMhz } from "./format";
import { formatDuration } from "./recordings";

export type LogView = "list" | LogGroupKey;

export const VIEW_CHOICES: { value: LogView; label: string }[] = [
  { value: "list", label: "List" },
  { value: "frequency", label: "Frequency" },
  { value: "station", label: "Station" },
];

export function viewOf(group: LogGroupKey | null | undefined): LogView {
  return group ?? "list";
}

export function groupOf(view: LogView): LogGroupKey | null {
  return view === "list" ? null : view;
}

export type GroupSort = "key" | "count" | "last";

export interface GroupRow {
  key: string;
  label: string;
  kind: string;
  count: number;
  airtimeMs: number | null;
  firstAt: string;
  lastAt: string;
  latest: LogRow;
}

export function groupRows(groups: readonly DecoderLogGroup[], by: LogGroupKey): GroupRow[] {
  return groups.map((group) => {
    const latest = storedRow(group.latest);
    return {
      key: groupKey(latest, by),
      label: by === "frequency" ? formatMhz(latest.freqHz) : (latest.station ?? "-"),
      kind: latest.kind,
      count: group.count,
      airtimeMs: group.airtime_ms ?? null,
      firstAt: group.first_at,
      lastAt: group.last_at,
      latest,
    };
  });
}

function groupKey(latest: LogRow, by: LogGroupKey): string {
  return by === "frequency"
    ? `frequency:${Math.round(latest.freqHz / 100)}`
    : `station:${latest.kind}:${latest.station ?? ""}`;
}

export function sortGroups(
  rows: readonly GroupRow[],
  sort: GroupSort,
  by: LogGroupKey,
): GroupRow[] {
  const recent = (a: GroupRow, b: GroupRow): number => timeMs(b.lastAt) - timeMs(a.lastAt);
  const order: Record<GroupSort, (a: GroupRow, b: GroupRow) => number> = {
    key: (a, b) =>
      by === "frequency" ? a.latest.freqHz - b.latest.freqHz : a.label.localeCompare(b.label),
    count: (a, b) => b.count - a.count,
    last: recent,
  };
  return rows.toSorted((a, b) => order[sort](a, b) || recent(a, b));
}

export function hasAirtime(rows: readonly GroupRow[]): boolean {
  return rows.some((row) => row.airtimeMs !== null);
}

export function formatAirtime(ms: number | null): string {
  return ms === null ? "-" : formatDuration(ms / 1000);
}

function timeMs(at: string): number {
  const ms = Date.parse(at);
  return Number.isNaN(ms) ? 0 : ms;
}
