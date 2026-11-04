import { useLayoutEffect, useRef } from "react";
import { Button } from "../components/BaseControls";
import { BTN, TABLE_CELL, TABLE_HEAD } from "../components/controls";
import {
  ageClass,
  buildTranscript,
  fieldColumns,
  fieldNamed,
  formatAge,
  isAtBottom,
} from "../components/decoderViews";
import { capturedImageUrl } from "../lib/api";
import { copyText } from "../lib/copyText";
import { kindHas, kindLabel, useEventKinds } from "../lib/eventKinds";
import { entityKey, groupsOf, lastSeenMs, type SinkGroup, type SinkView } from "../lib/sinks";
import { toastError } from "../lib/toasts";
import type { CapturedImage, Entity, EventKindEntry } from "../lib/types";
import { Fields } from "./Fields";
import { formatValue } from "./values";

const PANE = "flex flex-col gap-2 p-3";
const EMPTY = "text-sm text-ink-dim";

export function ReadoutView({
  feed,
  sourceName,
  now,
}: {
  feed: SinkView;
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
          <GroupView group={group} pictures={feed.pictures} entries={entries} now={now} />
        </section>
      ))}
    </>
  );
}

/** The view a kind's capabilities pick: stations, a transcript, pictures, else the latest record. */
export function GroupView({
  group,
  pictures,
  entries,
  now,
}: {
  group: SinkGroup;
  pictures: readonly CapturedImage[];
  entries: readonly EventKindEntry[];
  now: number;
}) {
  if (kindHas(group.kind, "station", entries) && group.entities.length > 0) {
    return <EntityTable entities={group.entities} now={now} />;
  }
  if (kindHas(group.kind, "text", entries)) {
    return <Transcript group={group} />;
  }
  const own = pictures.filter((picture) => picture.node === group.source);
  if (kindHas(group.kind, "picture", entries) && own.length > 0) {
    return <Pictures pictures={own} />;
  }
  return <LatestRecord group={group} />;
}

function EntityTable({ entities, now }: { entities: readonly Entity[]; now: number }) {
  const rows = entities.toSorted((a, b) => lastSeenMs(b) - lastSeenMs(a));
  const columns = fieldColumns(rows);
  return (
    <div className={`${PANE} overflow-x-auto`}>
      <table className="w-full border-collapse">
        <thead>
          <tr className="border-b border-line">
            <th className={TABLE_HEAD} scope="col">
              Name
            </th>
            {columns.length === 0 ? (
              <th className={TABLE_HEAD} scope="col">
                Last
              </th>
            ) : (
              columns.map((column) => (
                <th key={column} className={TABLE_HEAD} scope="col">
                  {column}
                </th>
              ))
            )}
            <th className={TABLE_HEAD} scope="col">
              Age
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((entity) => (
            <EntityRow
              key={entityKey(entity.key)}
              entity={entity}
              columns={columns}
              ageMs={Math.max(0, now - lastSeenMs(entity))}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function EntityRow({
  entity,
  columns,
  ageMs,
}: {
  entity: Entity;
  columns: readonly string[];
  ageMs: number;
}) {
  const fields = entity.view.fields;
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`} title={entity.view.summary}>
      <td className={`${TABLE_CELL} font-semibold`}>{entity.view.label ?? entity.key.id}</td>
      {columns.length === 0 ? (
        <td className={TABLE_CELL}>{entity.view.summary}</td>
      ) : (
        columns.map((column) => {
          const field = fieldNamed(fields, column);
          return (
            <td key={column} className={TABLE_CELL}>
              {field === undefined ? "-" : formatValue(field.value)}
            </td>
          );
        })
      )}
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

function Transcript({ group }: { group: SinkGroup }) {
  const text = buildTranscript(group.records);
  const paneRef = useRef<HTMLPreElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = paneRef.current;
    if (el !== null && stick.current) {
      el.scrollTop = el.scrollHeight;
    }
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- new text is what scrolls the pane
  }, [text]);

  return (
    <div className={PANE}>
      <div className="flex items-center gap-3">
        <Fields fields={group.records[0]?.view?.fields ?? []} />
        <Button
          type="button"
          className={`${BTN} ml-auto`}
          disabled={text === ""}
          onClick={() => {
            copyText(text).catch(toastError);
          }}
        >
          Copy all
        </Button>
      </div>
      <pre
        ref={paneRef}
        tabIndex={0}
        aria-label="Transcript"
        className="max-h-72 min-h-16 overflow-auto whitespace-pre-wrap break-words rounded-control border border-line bg-panel px-2 py-1.5 font-mono text-xs text-ink"
        onScroll={(e) => {
          stick.current = isAtBottom(e.currentTarget);
        }}
      >
        {text}
      </pre>
    </div>
  );
}

const PICTURES = 6;

function Pictures({ pictures }: { pictures: readonly CapturedImage[] }) {
  return (
    <div className="grid grid-cols-2 gap-1 p-3">
      {pictures.slice(0, PICTURES).map((picture) => {
        const title = `${picture.mode} ${picture.width} × ${picture.height}`;
        if (picture.image == null) {
          return (
            <span
              key={picture.id}
              className="text-2xs text-warn"
              title={picture.image_error ?? title}
            >
              {picture.mode}
            </span>
          );
        }
        const url = capturedImageUrl(picture.image.url);
        return (
          <a key={picture.id} href={url} target="_blank" rel="noreferrer" title={title}>
            <img src={url} alt={title} className="w-full rounded-inset object-contain" />
          </a>
        );
      })}
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
