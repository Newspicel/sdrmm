import { useState } from "react";
import { Button } from "../components/BaseControls";
import { BTN, TABLE_CELL, TABLE_HEAD } from "../components/controls";
import { ageClass, formatAge } from "../components/decoderViews";
import { kindHas, kindLabel, useEventKinds } from "../lib/eventKinds";
import { entityKey, groupsOf, lastSeenMs, type SinkGroup, type SinkView } from "../lib/sinks";
import type { Entity, EventKindEntry } from "../lib/types";
import { Fields } from "./Fields";
import { type Panels, renderPanel } from "./panels";

const PANE = "flex flex-col gap-2 p-3";
const EMPTY = "text-sm text-ink-dim";

export function ReadoutView({
  feed,
  panels,
  sourceName,
  now,
}: {
  feed: SinkView;
  panels: Panels;
  sourceName: (source: string) => string;
  now: number;
}) {
  const entries = useEventKinds();
  const groups = groupsOf(feed);
  if (groups.length === 0) {
    return <span className={`${EMPTY} p-3`}>Nothing arrived yet.</span>;
  }
  return (
    <>
      {groups.map((group) => (
        <section
          key={`${group.source}-${group.kind}`}
          className="border-b border-line last:border-b-0"
        >
          {groups.length > 1 && (
            <span className="legend block px-3 pt-2">
              {sourceName(group.source)} · {kindLabel(group.kind, entries)}
            </span>
          )}
          {renderPanel(panels, group) ?? <GenericGroup group={group} entries={entries} now={now} />}
        </section>
      ))}
    </>
  );
}

export function GenericGroup({
  group,
  entries,
  now,
}: {
  group: SinkGroup;
  entries: readonly EventKindEntry[];
  now: number;
}) {
  if (kindHas(group.kind, "station", entries) && group.entities.length > 0) {
    return <EntityTable entities={group.entities} now={now} />;
  }
  if (kindHas(group.kind, "text", entries)) {
    return <TextPane group={group} />;
  }
  return <LatestRecord group={group} />;
}

function EntityTable({ entities, now }: { entities: readonly Entity[]; now: number }) {
  const [open, setOpen] = useState<string | null>(null);
  const rows = entities.toSorted((a, b) => lastSeenMs(b) - lastSeenMs(a));
  return (
    <div className={`${PANE} overflow-x-auto`}>
      <table className="w-full border-collapse">
        <thead>
          <tr className="border-b border-line">
            <th className={TABLE_HEAD} scope="col">
              Name
            </th>
            <th className={TABLE_HEAD} scope="col">
              Last
            </th>
            <th className={TABLE_HEAD} scope="col">
              Age
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((entity) => {
            const key = entityKey(entity.key);
            const ageMs = Math.max(0, now - lastSeenMs(entity));
            return (
              <EntityRow
                key={key}
                entity={entity}
                ageMs={ageMs}
                open={open === key}
                onToggle={() => setOpen(open === key ? null : key)}
              />
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function EntityRow({
  entity,
  ageMs,
  open,
  onToggle,
}: {
  entity: Entity;
  ageMs: number;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <>
      <tr className={`border-b border-line/40 ${ageClass(ageMs)}`}>
        <td className={`${TABLE_CELL} font-semibold`}>
          <Button type="button" className={BTN} aria-expanded={open} onClick={onToggle}>
            {entity.view.label ?? entity.key.id}
          </Button>
        </td>
        <td className={TABLE_CELL}>{entity.view.summary}</td>
        <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
      </tr>
      {open && (
        <tr>
          <td colSpan={3} className="px-2 py-1.5">
            <Fields fields={entity.view.fields ?? []} />
          </td>
        </tr>
      )}
    </>
  );
}

function TextPane({ group }: { group: SinkGroup }) {
  const text = group.records
    .toReversed()
    .map((record) => record.view?.text ?? "")
    .join("");
  return (
    <div className={PANE}>
      <pre className="max-h-72 min-h-16 overflow-auto whitespace-pre-wrap break-words rounded-control border border-line bg-panel px-2 py-1.5 font-mono text-xs text-ink">
        {text}
      </pre>
    </div>
  );
}

function LatestRecord({ group }: { group: SinkGroup }) {
  const latest = group.records[0];
  if (latest?.view == null) {
    return <span className={`${EMPTY} p-3`}>Nothing yet.</span>;
  }
  return (
    <div className={PANE}>
      <span className="text-sm text-ink">{latest.view.summary}</span>
      <Fields fields={latest.view.fields ?? []} />
    </div>
  );
}
