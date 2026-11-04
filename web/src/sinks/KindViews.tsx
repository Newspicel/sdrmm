import { useState } from "react";
import { Button } from "../components/BaseControls";
import { TABLE_CELL, TABLE_HEAD } from "../components/controls";
import {
  ageClass,
  type FieldColumn,
  fieldKeyed,
  formatAge,
  namedColumns,
} from "../components/decoderViews";
import { MeterBar, MeterRow } from "../components/face/Meter";
import type { SinkGroup } from "../lib/sinks";
import type { CapturedImage, DecodedRecord, Field, Hue, KindView, SortKey } from "../lib/types";
import { HUE_COLOR } from "./hues";
import { Pictures } from "./Pictures";
import { formatNumber, formatValue } from "./values";
import {
  fieldNumber,
  type MeterView,
  meterLevel,
  newestField,
  nextSort,
  sortRows,
  type TableView,
  type TrendView,
  tableRows,
  trendLine,
  trendRange,
  trendSeries,
  type ViewRow,
} from "./viewData";

const PANE = "flex flex-col gap-2 p-3";
const EMPTY = "p-3 text-sm text-ink-dim";

/** Every layout a kind declares, in its order, over one source's events of that kind. */
export function KindViews({
  group,
  views,
  pictures,
  now,
}: {
  group: SinkGroup;
  views: readonly KindView[];
  pictures: readonly CapturedImage[];
  now: number;
}) {
  return views.map((view, index) => (
    <OneView
      key={`${view.type}-${index}`}
      view={view}
      group={group}
      pictures={pictures}
      now={now}
    />
  ));
}

function OneView({
  view,
  group,
  pictures,
  now,
}: {
  view: KindView;
  group: SinkGroup;
  pictures: readonly CapturedImage[];
  now: number;
}) {
  switch (view.type) {
    case "table":
      return <DeclaredTable group={group} view={view} now={now} />;
    case "meter":
      return <FieldMeter records={group.records} view={view} />;
    case "trend":
      return <FieldTrend records={group.records} view={view} now={now} />;
    case "picture":
      return pictures.length === 0 ? (
        <span className={`${EMPTY} block`}>No picture yet.</span>
      ) : (
        <Pictures pictures={pictures} limit={1} />
      );
  }
}

function DeclaredTable({ group, view, now }: { group: SinkGroup; view: TableView; now: number }) {
  const rows = tableRows(group, view.key);
  const columns = namedColumns(
    view.columns,
    rows.map((row) => row.fields),
  );
  return (
    <ViewTable
      rows={rows}
      columns={columns}
      named={view.key == null}
      initialSort={view.sort ?? null}
      now={now}
    />
  );
}

/** Rows under sortable field columns, a name column first when rows are entities, age last. */
export function ViewTable({
  rows,
  columns,
  named,
  initialSort,
  now,
}: {
  rows: readonly ViewRow[];
  columns: readonly FieldColumn[];
  named: boolean;
  initialSort: SortKey | null;
  now: number;
}) {
  const [sort, setSort] = useState(initialSort);
  if (rows.length === 0) {
    return <span className={`${EMPTY} block`}>Nothing yet.</span>;
  }
  return (
    <div className={`${PANE} overflow-x-auto`}>
      <table className="w-full border-collapse">
        <thead>
          <tr className="border-b border-line">
            {named && <PlainHead label="Name" />}
            {columns.length === 0 ? (
              <PlainHead label="Last" />
            ) : (
              columns.map((column) => (
                <SortHead
                  key={column.key}
                  column={column}
                  sort={sort}
                  onSort={() => setSort(nextSort(sort, column.key))}
                />
              ))
            )}
            <PlainHead label="Age" />
          </tr>
        </thead>
        <tbody>
          {sortRows(rows, sort).map((row) => (
            <TableRow
              key={row.id}
              row={row}
              columns={columns}
              named={named}
              ageMs={Math.max(0, now - row.seenMs)}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PlainHead({ label }: { label: string }) {
  return (
    <th className={TABLE_HEAD} scope="col">
      {label}
    </th>
  );
}

function SortHead({
  column,
  sort,
  onSort,
}: {
  column: FieldColumn;
  sort: SortKey | null;
  onSort: () => void;
}) {
  const active = sort?.field === column.key;
  const descending = sort?.descending === true;
  return (
    <th
      className={`${TABLE_HEAD} p-0`}
      scope="col"
      aria-sort={active ? (descending ? "descending" : "ascending") : "none"}
    >
      <Button
        type="button"
        className={`block w-full truncate px-2 py-1 text-left hover:text-ink focus-visible:outline focus-visible:outline-accent ${
          active ? "text-ink" : ""
        }`}
        onClick={onSort}
      >
        {column.label}
        {active ? (descending ? " ↓" : " ↑") : ""}
      </Button>
    </th>
  );
}

function TableRow({
  row,
  columns,
  named,
  ageMs,
}: {
  row: ViewRow;
  columns: readonly FieldColumn[];
  named: boolean;
  ageMs: number;
}) {
  return (
    <tr className={`border-b border-line/40 ${ageClass(ageMs)}`} title={row.summary}>
      {named && <td className={`${TABLE_CELL} font-semibold`}>{row.name}</td>}
      {columns.length === 0 ? (
        <td className={TABLE_CELL}>{row.summary}</td>
      ) : (
        columns.map((column) => {
          const field = fieldKeyed(row.fields, column.key);
          return (
            <td key={column.key} className={TABLE_CELL}>
              {field === undefined ? "-" : formatValue(field.value)}
            </td>
          );
        })
      )}
      <td className={`${TABLE_CELL} text-right`}>{formatAge(ageMs)}</td>
    </tr>
  );
}

function FieldMeter({ records, view }: { records: readonly DecodedRecord[]; view: MeterView }) {
  const field = newestField(records, view.field);
  const value = fieldNumber(field);
  if (field === undefined || value === null) {
    return <span className={`${EMPTY} block`}>No reading yet.</span>;
  }
  const text = formatValue(field.value);
  return (
    <div className="px-3 py-2">
      <MeterRow
        label={field.label}
        title={`${field.label}, ${rangeText(field, view)}`}
        meter={
          <MeterBar
            label={field.label}
            value={meterLevel(value, view.min, view.max)}
            valueText={text}
          />
        }
        readout={text}
      />
    </div>
  );
}

function rangeText(field: Field, view: MeterView): string {
  if (field.value.type !== "number") {
    return `${view.min} to ${view.max}`;
  }
  const { unit, digits } = field.value.value;
  return `${formatNumber(view.min, unit, digits)} to ${formatNumber(view.max, unit, digits)}`;
}

const TREND_W = 240;
const TREND_H = 56;
const SERIES_HUES: readonly Hue[] = ["accent", "amber", "cyan", "pink", "green", "violet"];

function seriesColor(index: number): string {
  return HUE_COLOR[SERIES_HUES[index % SERIES_HUES.length] ?? "accent"];
}

function FieldTrend({
  records,
  view,
  now,
}: {
  records: readonly DecodedRecord[];
  view: TrendView;
  now: number;
}) {
  const series = trendSeries(records, view.fields, view.window_s, now);
  const range = trendRange(series);
  if (range === null) {
    return <span className={`${EMPTY} block`}>No reading yet.</span>;
  }
  const window = { windowS: view.window_s, nowMs: now };
  return (
    <div className={PANE}>
      <svg
        viewBox={`0 0 ${TREND_W} ${TREND_H}`}
        preserveAspectRatio="none"
        className="h-14 w-full rounded-inset border border-line bg-panel"
        role="img"
        aria-label={`Last ${view.window_s} s`}
      >
        {series.map((one, index) => (
          <polyline
            key={one.key}
            points={trendLine(one.points, range, window, { width: TREND_W, height: TREND_H })}
            fill="none"
            stroke={seriesColor(index)}
            strokeWidth={1.5}
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </svg>
      <span className="flex flex-wrap gap-3 font-mono text-2xs text-ink-faint">
        {series.map((one, index) => (
          <span key={one.key} className="inline-flex items-center gap-1">
            <span
              aria-hidden
              className="size-2 rounded-full"
              style={{ background: seriesColor(index) }}
            />
            {one.label}
            <b className="font-medium text-ink">{newestText(records, one.key)}</b>
          </span>
        ))}
      </span>
    </div>
  );
}

function newestText(records: readonly DecodedRecord[], key: string): string {
  const field = newestField(records, key);
  return field === undefined ? "-" : formatValue(field.value);
}
