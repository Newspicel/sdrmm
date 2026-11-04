import { useLayoutEffect, useRef } from "react";
import { Button } from "../components/BaseControls";
import { BTN } from "../components/controls";
import { buildTranscript, fieldColumns, isAtBottom } from "../components/decoderViews";
import { copyText } from "../lib/copyText";
import { kindHas, kindLabel, kindViews, useEventKinds } from "../lib/eventKinds";
import { groupsOf, type SinkGroup, type SinkView } from "../lib/sinks";
import { toastError } from "../lib/toasts";
import type { CapturedImage, EventKindEntry } from "../lib/types";
import { Fields } from "./Fields";
import { FileView } from "./FileView";
import { KindViews, ViewTable } from "./KindViews";
import { Pictures } from "./Pictures";
import { tableRows } from "./viewData";

const PANE = "flex flex-col gap-2 p-3";
const EMPTY = "text-sm text-ink-dim";
const PICTURES = 6;

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

/**
 * The views a kind declares, else the one its capabilities pick: stations, a transcript,
 * pictures, else the latest record.
 */
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
  const own = pictures.filter((picture) => picture.node === group.source);
  const views = kindViews(group.kind, entries);
  if (views.length > 0) {
    return <KindViews group={group} views={views} pictures={own} now={now} />;
  }
  if (kindHas(group.kind, "station", entries) && group.entities.length > 0) {
    return (
      <ViewTable
        rows={tableRows(group, null)}
        columns={fieldColumns(group.entities)}
        named
        initialSort={null}
        now={now}
      />
    );
  }
  if (kindHas(group.kind, "text", entries)) {
    return <Transcript group={group} />;
  }
  if (kindHas(group.kind, "picture", entries) && own.length > 0) {
    return <Pictures pictures={own} limit={PICTURES} />;
  }
  return <LatestRecord group={group} />;
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

function LatestRecord({ group }: { group: SinkGroup }) {
  const latest = group.records[0];
  if (latest?.view == null) {
    return <span className={`${EMPTY} p-3`}>Nothing yet.</span>;
  }
  return (
    <div className={PANE}>
      <span className="text-sm text-ink">{latest.view.summary}</span>
      <Fields fields={latest.view.fields ?? []} />
      {latest.view.file != null && <FileView file={latest.view.file} />}
    </div>
  );
}
