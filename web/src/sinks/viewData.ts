import { fieldKeyed } from "../components/decoderViews";
import { entityKey, lastSeenMs, type SinkGroup } from "../lib/sinks";
import type { DecodedRecord, Entity, Field, KindView, SortKey } from "../lib/types";

export type TableView = Extract<KindView, { type: "table" }>;
export type MeterView = Extract<KindView, { type: "meter" }>;
export type TrendView = Extract<KindView, { type: "trend" }>;

/** One table row: an entity with the newest value of each field it carried. */
export interface ViewRow {
  id: string;
  name: string;
  summary: string;
  seenMs: number;
  fields: readonly Field[];
}

/** `newer` plus each field of `older` whose key `newer` lacks. */
export function mergeFields(newer: readonly Field[], older: readonly Field[]): Field[] {
  const held = new Set(newer.map((field) => field.key));
  return [...newer, ...older.filter((field) => !held.has(field.key))];
}

function entityRow(entity: Entity): ViewRow {
  return {
    id: entityKey(entity.key),
    name: entity.view.label ?? entity.key.id,
    summary: entity.view.summary,
    seenMs: lastSeenMs(entity),
    fields: entity.view.fields ?? [],
  };
}

function atMs(record: Pick<DecodedRecord, "at">): number {
  const at = Date.parse(record.at);
  return Number.isNaN(at) ? 0 : at;
}

function rowIdOf(record: DecodedRecord, key: string | null | undefined): string | null {
  if (key == null) {
    return record.view?.entity ?? null;
  }
  const field = fieldKeyed(record.view?.fields, key);
  return field === undefined ? null : JSON.stringify(field.value);
}

/** Rows from newest-first records, one per `key` value, else per presentation entity. */
export function recordRows(
  records: readonly DecodedRecord[],
  key: string | null | undefined,
): ViewRow[] {
  const rows = new Map<string, ViewRow>();
  for (const record of records) {
    const id = rowIdOf(record, key);
    const view = record.view;
    if (id === null || view == null) {
      continue;
    }
    const row = rows.get(id);
    const fields = view.fields ?? [];
    rows.set(
      id,
      row === undefined
        ? {
            id,
            name: view.label ?? view.entity ?? id,
            summary: view.summary,
            seenMs: atMs(record),
            fields,
          }
        : { ...row, fields: mergeFields(row.fields, fields) },
    );
  }
  return [...rows.values()];
}

/** A table's rows: the sink's entities while the kind keys by entity, else rows from records. */
export function tableRows(group: SinkGroup, key: string | null | undefined): ViewRow[] {
  if (key == null && group.entities.length > 0) {
    return group.entities.map(entityRow);
  }
  return recordRows(group.records, key);
}

type SortValue = number | string | null;

function sortValue(field: Field | undefined): SortValue {
  switch (field?.value.type) {
    case "number":
      return field.value.value.value;
    case "flag":
      return field.value.value ? 1 : 0;
    case "time":
      return Date.parse(field.value.value);
    case "text":
      return field.value.value;
    default:
      return null;
  }
}

function compareValues(a: SortValue, b: SortValue): number {
  if (typeof a === "number" && typeof b === "number") {
    return a - b;
  }
  return String(a).localeCompare(String(b), undefined, { numeric: true });
}

/** Rows ordered by `sort`, rows lacking the field last; newest first without one. */
export function sortRows(rows: readonly ViewRow[], sort: SortKey | null): ViewRow[] {
  if (sort === null) {
    return rows.toSorted((a, b) => b.seenMs - a.seenMs);
  }
  const sign = sort.descending === true ? -1 : 1;
  return rows.toSorted((a, b) => {
    const left = sortValue(fieldKeyed(a.fields, sort.field));
    const right = sortValue(fieldKeyed(b.fields, sort.field));
    if (left === null || right === null) {
      return (left === null ? 1 : 0) - (right === null ? 1 : 0);
    }
    return sign * compareValues(left, right);
  });
}

/** The sort a header click picks: the same column flips, another sorts ascending. */
export function nextSort(current: SortKey | null, field: string): SortKey {
  if (current?.field === field) {
    return { field, descending: current.descending !== true };
  }
  return { field, descending: false };
}

export function fieldNumber(field: Field | undefined): number | null {
  return field?.value.type === "number" ? field.value.value.value : null;
}

/** The newest field keyed `key` among newest-first records. */
export function newestField(records: readonly DecodedRecord[], key: string): Field | undefined {
  for (const record of records) {
    const field = fieldKeyed(record.view?.fields, key);
    if (field !== undefined) {
      return field;
    }
  }
  return undefined;
}

/** Where `value` sits on `min..max`, clamped to 0..1. */
export function meterLevel(value: number, min: number, max: number): number {
  if (!(max > min)) {
    return 0;
  }
  return Math.min(1, Math.max(0, (value - min) / (max - min)));
}

export interface TrendPoint {
  atMs: number;
  value: number;
}

export interface TrendSeries {
  key: string;
  label: string;
  points: TrendPoint[];
}

/** Each numeric field over the last `windowS` seconds, oldest first. */
export function trendSeries(
  records: readonly DecodedRecord[],
  keys: readonly string[],
  windowS: number,
  nowMs: number,
): TrendSeries[] {
  const since = nowMs - windowS * 1000;
  const recent = records.filter((record) => atMs(record) >= since).toReversed();
  return keys.map((key) => {
    let label = key;
    const points: TrendPoint[] = [];
    for (const record of recent) {
      const field = fieldKeyed(record.view?.fields, key);
      const value = fieldNumber(field);
      if (field !== undefined && value !== null) {
        label = field.label;
        points.push({ atMs: atMs(record), value });
      }
    }
    return { key, label, points };
  });
}

export interface TrendRange {
  lo: number;
  hi: number;
}

/** The value range every series spans, widened when flat so a line still draws. */
export function trendRange(series: readonly TrendSeries[]): TrendRange | null {
  const values = series.flatMap((one) => one.points.map((point) => point.value));
  if (values.length === 0) {
    return null;
  }
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  return lo === hi ? { lo: lo - 1, hi: hi + 1 } : { lo, hi };
}

/** SVG polyline points for a series on a `width` by `height` box. */
export function trendLine(
  points: readonly TrendPoint[],
  range: TrendRange,
  window: { windowS: number; nowMs: number },
  size: { width: number; height: number },
): string {
  const spanMs = Math.max(1, window.windowS * 1000);
  const start = window.nowMs - spanMs;
  return points
    .map((point) => {
      const x = ((point.atMs - start) / spanMs) * size.width;
      const y = (1 - (point.value - range.lo) / (range.hi - range.lo)) * size.height;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}
