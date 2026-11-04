import type { DecodedRecord, Entity, Field } from "../lib/types";

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

/** The text of newest-first records, oldest first. */
export function buildTranscript(
  records: readonly Pick<DecodedRecord, "view">[],
  limit = TRANSCRIPT_LIMIT,
): string {
  return records.reduceRight(
    (text, record) => appendTranscript(text, record.view?.text ?? "", limit),
    "",
  );
}

export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

export function isAtBottom(m: ScrollMetrics, tolerancePx = 8): boolean {
  return m.scrollHeight - m.scrollTop - m.clientHeight <= tolerancePx;
}

/** A table column: the field `key` it reads and the `label` it shows. */
export interface FieldColumn {
  key: string;
  label: string;
}

/** Every field the entities show, by key, in the order they first appear. */
export function fieldColumns(entities: readonly Pick<Entity, "view">[]): FieldColumn[] {
  return keyedColumns(entities.map((entity) => entity.view.fields ?? []));
}

function keyedColumns(fieldLists: readonly (readonly Field[])[]): FieldColumn[] {
  const labels = new Map<string, string>();
  for (const fields of fieldLists) {
    for (const field of fields) {
      if (!labels.has(field.key)) {
        labels.set(field.key, field.label);
      }
    }
  }
  return [...labels].map(([key, label]) => ({ key, label }));
}

/** The columns `keys` names, each headed by the label its field carries, else its key. */
export function namedColumns(
  keys: readonly string[],
  fieldLists: readonly (readonly Field[])[],
): FieldColumn[] {
  const labels = new Map(keyedColumns(fieldLists).map((column) => [column.key, column.label]));
  return keys.map((key) => ({ key, label: labels.get(key) ?? key }));
}

export function fieldKeyed(fields: readonly Field[] | undefined, key: string): Field | undefined {
  return fields?.find((field) => field.key === key);
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}
