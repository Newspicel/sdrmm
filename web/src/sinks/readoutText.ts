import { buildTranscript, formatClock } from "../components/decoderViews";
import { kindHas, kindLabel } from "../lib/eventKinds";
import { lastSeenMs, type SinkGroup } from "../lib/sinks";
import type { DecodedRecord, Entity, EventKindEntry, Field } from "../lib/types";
import { formatValue } from "./values";

/** One wired source and kind as the readout sidebar lists it. */
export interface SourceRow {
  key: string;
  label: string;
  count: number;
  lastMs: number | null;
}

export function sourceRows(
  groups: readonly SinkGroup[],
  entries: readonly EventKindEntry[],
  sourceName: (source: string) => string,
): SourceRow[] {
  return groups.map((group) => ({
    key: `${group.source}-${group.kind}`,
    label: `${sourceName(group.source)} · ${kindLabel(group.kind, entries)}`,
    count: group.records.length + group.entities.length,
    lastMs: lastOf(group),
  }));
}

function lastOf(group: SinkGroup): number | null {
  const times = [
    ...group.records.map((record) => Date.parse(record.at)),
    ...group.entities.map(lastSeenMs),
  ].filter((at) => !Number.isNaN(at));
  return times.length === 0 ? null : Math.max(...times);
}

/** Everything the readout shows, as plain text: a transcript, a line per station, else a line per record. */
export function readoutText(
  groups: readonly SinkGroup[],
  entries: readonly EventKindEntry[],
  sourceName: (source: string) => string,
): string {
  const titled = groups.length > 1;
  return groups
    .map((group) => {
      const body = groupText(group, entries);
      if (body === "" || !titled) {
        return body;
      }
      return `${sourceName(group.source)} · ${kindLabel(group.kind, entries)}\n${body}`;
    })
    .filter((text) => text !== "")
    .join("\n\n");
}

function groupText(group: SinkGroup, entries: readonly EventKindEntry[]): string {
  if (kindHas(group.kind, "text", entries)) {
    return buildTranscript(group.records);
  }
  if (group.entities.length > 0) {
    return group.entities.map(entityLine).join("\n");
  }
  return group.records.toReversed().map(recordLine).join("\n");
}

function entityLine(entity: Entity): string {
  return line(entity.view.label ?? entity.key.id, entity.view.summary, entity.view.fields);
}

function recordLine(record: DecodedRecord): string {
  return line(formatClock(record.at), record.view?.summary ?? "", record.view?.fields);
}

function line(head: string, summary: string, fields: readonly Field[] = []): string {
  const values = fields.map((field) => `${field.label} ${formatValue(field.value)}`).join(", ");
  return [head, summary, values].filter((part) => part !== "").join("  ");
}
