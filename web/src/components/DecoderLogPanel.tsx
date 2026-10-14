import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type RefObject, useEffect, useMemo, useRef, useState } from "react";
import { FaceBody, FaceFooter } from "../canvas/nodes/NodeShell";
import {
  clearDecoderLog,
  DECODER_LOG_KEY,
  decoderLogGroupsQuery,
  decoderLogQuery,
} from "../lib/api";
import { useKindLabel } from "../lib/eventKinds";
import { useSinkFeed } from "../lib/sinks";
import { toastError } from "../lib/toasts";
import type { LogGroupKey } from "../lib/types";
import { Input } from "./BaseControls";
import { ConfirmButton } from "./ConfirmButton";
import { FIELD_SM, TABLE_CELL, TABLE_HEAD, TABLE_TEXT, tableRow } from "./controls";
import { DecoderLogGroups } from "./DecoderLogGroups";
import { DownloadMenu } from "./DownloadMenu";
import {
  buildRows,
  COLUMN_STEP,
  type ColumnWidths,
  callsignTitle,
  collectLive,
  DEFAULT_LOG_FILTER,
  droppedNotice,
  FLEX_COLUMN,
  isFiltered,
  LIMIT_OPTIONS,
  LOG_COLUMNS,
  type LogFilter,
  type LogRow,
  logDownloads,
  readColumnWidths,
  resizeColumn,
  rowSignature,
  toQuery,
  totalColumnWidth,
  type WireScope,
  writeColumnWidths,
} from "./decoderLog";
import { formatClock } from "./decoderViews";
import { ChoiceChip } from "./face/Chips";
import { FaceFault } from "./face/Fault";
import { FaceStats, Stat } from "./face/Stats";
import { formatMhz } from "./format";
import { RowDetail } from "./LogRowDetail";
import { groupOf, groupRows, VIEW_CHOICES, viewOf } from "./logGroups";
import { useRowAnchor } from "./rowAnchor";

const SEARCH_DEBOUNCE_MS = 250;

function newestMs(records: readonly { at: string }[]): number {
  const at = Date.parse(records[0]?.at ?? "");
  return Number.isNaN(at) ? 0 : at;
}

const LIMIT_CHOICES = LIMIT_OPTIONS.map((n) => ({ value: n, label: String(n) }));

export function DecoderLogPanel({
  wires,
  group,
  onGroup,
}: {
  wires: WireScope;
  group: LogGroupKey | null;
  onGroup: (group: LogGroupKey | null) => void;
}) {
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<LogFilter>(DEFAULT_LOG_FILTER);
  const [search, setSearch] = useState(DEFAULT_LOG_FILTER.q);
  const [cleared, setCleared] = useState<number | null>(null);
  const [widths, setWidths] = useState<ColumnWidths>(readColumnWidths);
  const commit = (): void => writeColumnWidths(widths);
  const scroller = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (search === filter.q) {
      return;
    }
    const timer = window.setTimeout(
      () => setFilter((f) => ({ ...f, q: search })),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [search, filter.q]);

  const query = toQuery(filter, wires);
  const log = useQuery({ ...decoderLogQuery(query), enabled: group === null });
  const grouped = useQuery({
    ...decoderLogGroupsQuery(query, group ?? "frequency"),
    enabled: group !== null,
  });
  const groups = useMemo(
    () => (group === null ? [] : groupRows(grouped.data?.groups ?? [], group)),
    [grouped.data, group],
  );
  const feed = useSinkFeed(wires.sink);
  const lost = feed.lost;
  const [clearedAt, setClearedAt] = useState(0);

  const entries = log.data?.entries;
  const rows = useMemo(
    () => buildRows(entries ?? [], collectLive(feed.records, filter, clearedAt)),
    [entries, feed.records, filter, clearedAt],
  );

  const clearMut = useMutation({
    mutationFn: () => clearDecoderLog(query),
    onSuccess: (deleted) => {
      const live = rows.filter((row) => row.live).length;
      setClearedAt(newestMs(feed.records));
      setCleared(deleted + live);
    },
    onError: toastError,
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: DECODER_LOG_KEY });
    },
  });

  const patch = (next: Partial<LogFilter>): void => {
    setCleared(null);
    setFilter((f) => ({ ...f, ...next }));
  };

  const source = group === null ? log : grouped;
  const droppedRows = source.data?.dropped ?? 0;
  const dropped = droppedNotice(lost, droppedRows);
  const total = source.data?.total ?? 0;
  const shown = group === null ? rows.length : groups.length;

  return (
    <>
      <FaceBody scroll={false}>
        <div className="flex min-h-0 flex-1 flex-col gap-2 p-2">
          <div className="flex items-center gap-2">
            <Input
              className={`${FIELD_SM} min-w-0 flex-1`}
              placeholder="Search station or summary"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              aria-label="Search decoder log"
            />
            <ChoiceChip
              label="Group"
              title="One row per frequency or station"
              value={viewOf(group)}
              options={VIEW_CHOICES}
              onChange={(view) => onGroup(groupOf(view))}
            />
            <ChoiceChip
              label="Rows"
              title="Row limit"
              value={filter.limit}
              options={LIMIT_CHOICES}
              onChange={(limit) => patch({ limit })}
            />
          </div>

          <div
            ref={scroller}
            className="min-h-0 flex-1 overflow-auto rounded-control border border-line [overflow-anchor:none]"
          >
            {group === null ? (
              <LogTable
                rows={rows}
                scroller={scroller}
                widths={widths}
                setWidths={setWidths}
                commit={commit}
              />
            ) : (
              <DecoderLogGroups rows={groups} by={group} scroller={scroller} />
            )}

            {shown === 0 && (
              <div className="px-3 py-2 text-sm text-ink-dim">
                {source.isPending
                  ? "Loading…"
                  : isFiltered(filter)
                    ? "No rows match this filter."
                    : "Nothing logged yet."}
              </div>
            )}
          </div>
        </div>
        {source.isError && <FaceFault message={`Log unavailable: ${source.error.message}`} />}
      </FaceBody>
      <FaceFooter>
        <FaceStats>
          <Stat label="Shown" title="Rows in this view">
            {shown}
          </Stat>
          <Stat
            label={group === null ? "Stored" : "Groups"}
            title={group === null ? "Rows stored for this node" : "Groups in the stored rows"}
          >
            {total}
          </Stat>
          {cleared !== null && (
            <Stat label="Cleared" title="Rows removed by the last clear">
              {cleared}
            </Stat>
          )}
          {dropped !== null && (
            <Stat label="Dropped" title={dropped} tone="warn">
              {lost + droppedRows}
            </Stat>
          )}
        </FaceStats>
        <DownloadMenu choices={logDownloads(query)} />
        <ConfirmButton
          label="Clear"
          title="Remove every stored row this node can see"
          disabled={clearMut.isPending}
          onConfirm={() => clearMut.mutate()}
        />
      </FaceFooter>
    </>
  );
}

function LogTable({
  rows,
  scroller,
  widths,
  setWidths,
  commit,
}: {
  rows: readonly LogRow[];
  scroller: RefObject<HTMLDivElement | null>;
  widths: ColumnWidths;
  setWidths: (update: (widths: ColumnWidths) => ColumnWidths) => void;
  commit: () => void;
}) {
  const [opened, setOpened] = useState<string | null>(null);
  const anchor = useRowAnchor(scroller, opened);
  return (
    <table
      className="table-fixed border-collapse text-xs"
      style={{ width: totalColumnWidth(widths), minWidth: "100%" }}
    >
      <colgroup>
        {LOG_COLUMNS.map((column) => (
          <col
            key={column.key}
            style={column.key === FLEX_COLUMN ? undefined : { width: widths[column.key] }}
          />
        ))}
      </colgroup>
      <thead className="sticky top-0 bg-panel">
        <tr className="border-b border-line">
          {LOG_COLUMNS.map((column) => {
            const last = column.key === FLEX_COLUMN;
            return (
              <th
                key={column.key}
                className={`${TABLE_HEAD} relative ${column.key === "freq" ? "text-right" : ""}`}
              >
                <span className="block truncate">{column.label}</span>
                {!last && (
                  <ColumnHandle
                    label={column.label}
                    width={widths[column.key]}
                    onResize={(px) => setWidths((w) => resizeColumn(w, column.key, px))}
                    onCommit={commit}
                  />
                )}
              </th>
            );
          })}
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => {
          const id = rowSignature(row);
          const open = opened === id;
          return (
            <LogRows
              key={row.key}
              row={row}
              striped={index % 2 === 1}
              open={open}
              anchor={open ? anchor : undefined}
              onToggle={() => setOpened(open ? null : id)}
            />
          );
        })}
      </tbody>
    </table>
  );
}

function ColumnHandle({
  label,
  width,
  onResize,
  onCommit,
}: {
  label: string;
  width: number;
  onResize: (px: number) => void;
  onCommit: () => void;
}) {
  const drag = useRef<{ x: number; width: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  return (
    <span
      role="separator"
      aria-orientation="vertical"
      aria-label={`Resize ${label} column`}
      tabIndex={0}
      className="group absolute inset-y-0 -right-[5px] z-10 flex w-[9px] cursor-col-resize touch-none justify-center focus-visible:outline-none"
      onPointerDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
        drag.current = { x: event.clientX, width };
        setDragging(true);
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        const start = drag.current;
        if (start !== null) {
          onResize(start.width + event.clientX - start.x);
        }
      }}
      onPointerUp={(event) => {
        if (drag.current !== null) {
          drag.current = null;
          event.currentTarget.releasePointerCapture(event.pointerId);
          onCommit();
        }
      }}
      onLostPointerCapture={() => {
        drag.current = null;
        setDragging(false);
      }}
      onKeyDown={(event) => {
        const step =
          event.key === "ArrowLeft" ? -COLUMN_STEP : event.key === "ArrowRight" ? COLUMN_STEP : 0;
        if (step !== 0) {
          event.preventDefault();
          onResize(width + step);
          onCommit();
        }
      }}
    >
      <span
        className={`w-[3px] transition-colors group-hover:bg-accent group-focus-visible:bg-accent ${
          dragging ? "bg-accent" : "bg-transparent"
        }`}
      />
    </span>
  );
}

function LogRows({
  row,
  striped,
  open,
  anchor,
  onToggle,
}: {
  row: LogRow;
  striped: boolean;
  open: boolean;
  anchor?: RefObject<HTMLTableRowElement | null>;
  onToggle: () => void;
}) {
  const kindLabel = useKindLabel();
  return (
    <>
      <tr
        ref={anchor}
        className={tableRow(striped)}
        aria-expanded={open}
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onToggle();
          }
        }}
      >
        <td className={`${TABLE_CELL} truncate tabular-nums text-ink-dim`} title={row.at}>
          {formatClock(row.at)}
        </td>
        <td className={`${TABLE_TEXT} truncate text-ink-dim`}>{kindLabel(row.kind)}</td>
        <td className={`${TABLE_CELL} truncate text-right tabular-nums text-ink`}>
          {formatMhz(row.freqHz)}
        </td>
        <td
          className={`${TABLE_CELL} truncate text-ink`}
          title={row.callsign === null ? (row.station ?? undefined) : callsignTitle(row.callsign)}
        >
          {row.station ?? "-"}
          {row.callsign !== null && (
            <span className="ml-1 text-ink-dim">{row.callsign.country}</span>
          )}
        </td>
        <td className={`${TABLE_TEXT} truncate text-ink`} title={row.summary}>
          {row.summary}
        </td>
      </tr>
      {open && (
        <tr className="bg-panel-2">
          <td colSpan={LOG_COLUMNS.length} className="px-3 py-2">
            <RowDetail row={row} />
          </td>
        </tr>
      )}
    </>
  );
}
