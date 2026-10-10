import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { FaceBody, FaceFooter } from "../canvas/nodes/NodeShell";
import {
  clearDecoderLog,
  DECODER_LOG_KEY,
  decoderLogGroupsQuery,
  decoderLogQuery,
} from "../lib/api";
import { useDecodedStore } from "../lib/decoded";
import type { LogGroupKey } from "../lib/types";
import { Button, Input } from "./BaseControls";
import { BTN, FIELD, TABLE_CELL, TABLE_HEAD } from "./controls";
import { DecoderLogGroups } from "./DecoderLogGroups";
import { DownloadMenu } from "./DownloadMenu";
import {
  buildRows,
  COLUMN_STEP,
  type ColumnWidths,
  collectLive,
  DEFAULT_LOG_FILTER,
  droppedNotice,
  FLEX_COLUMN,
  isFiltered,
  kindLabel,
  LIMIT_OPTIONS,
  LOG_COLUMNS,
  type LogFilter,
  type LogRow,
  logDownloads,
  matchesFilter,
  readColumnWidths,
  resizeColumn,
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

const SEARCH_DEBOUNCE_MS = 250;
const CLEAR_ARM_MS = 3000;

const NO_FRAMES = {};

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
  const [armed, setArmed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cleared, setCleared] = useState<number | null>(null);
  const [widths, setWidths] = useState<ColumnWidths>(readColumnWidths);
  const commit = (): void => writeColumnWidths(widths);

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

  useEffect(() => {
    if (!armed) {
      return;
    }
    const timer = window.setTimeout(() => setArmed(false), CLEAR_ARM_MS);
    return () => window.clearTimeout(timer);
  }, [armed]);

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
  const frames = useDecodedStore((s) => (wires.wired ? s.frames : NO_FRAMES));
  const lost = useDecodedStore((s) => s.lost);

  const entries = log.data?.entries;
  const rows = useMemo(
    () => buildRows(entries ?? [], collectLive(frames, filter, wires.sink)),
    [entries, frames, filter, wires.sink],
  );

  const clearMut = useMutation({
    mutationFn: () => clearDecoderLog(query),
    onSuccess: (deleted) => {
      const live = rows.filter((row) => row.live).length;
      useDecodedStore.getState().dropFrames((record) => matchesFilter(record, filter, wires.sink));
      setError(null);
      setCleared(deleted + live);
    },
    onError: (e) => setError(e.message),
    onSettled: () => {
      setArmed(false);
      void queryClient.invalidateQueries({ queryKey: DECODER_LOG_KEY });
    },
  });

  const patch = (next: Partial<LogFilter>): void => {
    setCleared(null);
    setError(null);
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
              className={`${FIELD} min-w-0 flex-1`}
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

          <div className="min-h-0 flex-1 overflow-auto rounded border border-line">
            {group === null ? (
              <LogTable rows={rows} widths={widths} setWidths={setWidths} commit={commit} />
            ) : (
              <DecoderLogGroups rows={groups} by={group} />
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
        {error !== null && <FaceFault message={`Rejected: ${error}`} />}
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
        <Button
          type="button"
          className={`${BTN} hover:border-danger hover:text-danger ${
            armed ? "border-danger text-danger" : ""
          }`}
          disabled={clearMut.isPending}
          title={armed ? "Removes every stored row this node can see" : undefined}
          onClick={() => (armed ? clearMut.mutate() : setArmed(true))}
        >
          {armed ? "Confirm clear" : "Clear"}
        </Button>
      </FaceFooter>
    </>
  );
}

function LogTable({
  rows,
  widths,
  setWidths,
  commit,
}: {
  rows: readonly LogRow[];
  widths: ColumnWidths;
  setWidths: (update: (widths: ColumnWidths) => ColumnWidths) => void;
  commit: () => void;
}) {
  const [opened, setOpened] = useState<string | null>(null);
  return (
    <table
      className="table-fixed border-collapse font-mono text-xs"
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
                className={`${TABLE_HEAD} relative ${last ? "" : "border-r border-line"} ${
                  column.key === "freq" ? "text-right" : ""
                }`}
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
        {rows.map((row) => (
          <LogRows
            key={row.key}
            row={row}
            open={opened === row.key}
            onToggle={() => setOpened(opened === row.key ? null : row.key)}
          />
        ))}
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

function LogRows({ row, open, onToggle }: { row: LogRow; open: boolean; onToggle: () => void }) {
  return (
    <>
      <tr
        className="cursor-pointer border-b border-line/50 hover:bg-panel-2 focus-visible:outline focus-visible:outline-accent"
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
        <td className={`${TABLE_CELL} truncate text-ink-dim`}>{kindLabel(row.kind)}</td>
        <td className={`${TABLE_CELL} truncate text-right tabular-nums text-ink`}>
          {formatMhz(row.freqHz)}
        </td>
        <td className={`${TABLE_CELL} truncate text-ink`} title={row.station ?? undefined}>
          {row.station ?? "-"}
        </td>
        <td className={`${TABLE_CELL} truncate text-ink`} title={row.summary}>
          {row.summary}
        </td>
      </tr>
      {open && (
        <tr className="border-b border-line/50 bg-panel-2">
          <td colSpan={LOG_COLUMNS.length} className="px-3 py-2">
            <RowDetail row={row} />
          </td>
        </tr>
      )}
    </>
  );
}
