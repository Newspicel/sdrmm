import { useState } from "react";
import type { LogGroupKey } from "../lib/types";
import { Button } from "./BaseControls";
import { TABLE_CELL, TABLE_HEAD } from "./controls";
import { kindLabel } from "./decoderLog";
import { formatClock } from "./decoderViews";
import { RowDetail } from "./LogRowDetail";
import { formatAirtime, type GroupRow, type GroupSort, hasAirtime, sortGroups } from "./logGroups";

interface Column {
  key: string;
  label: string;
  width?: number;
  sort?: GroupSort;
  right?: boolean;
}

function columnsFor(by: LogGroupKey, airtime: boolean): Column[] {
  const columns: Column[] = [
    {
      key: "key",
      label: by === "frequency" ? "Frequency" : "Station",
      width: by === "frequency" ? 116 : 152,
      sort: "key",
      right: by === "frequency",
    },
    { key: "kind", label: "Kind", width: 96 },
    { key: "count", label: "Hits", width: 76, sort: "count", right: true },
  ];
  if (airtime) {
    columns.push({ key: "airtime", label: "Airtime", width: 72, right: true });
  }
  columns.push(
    { key: "last", label: "Last", width: 84, sort: "last" },
    { key: "latest", label: "Latest" },
  );
  return columns;
}

export function DecoderLogGroups({ rows, by }: { rows: readonly GroupRow[]; by: LogGroupKey }) {
  const [sort, setSort] = useState<GroupSort>("last");
  const [opened, setOpened] = useState<string | null>(null);
  const airtime = hasAirtime(rows);
  const columns = columnsFor(by, airtime);
  return (
    <table className="w-full table-fixed border-collapse font-mono text-xs">
      <colgroup>
        {columns.map((column) => (
          <col
            key={column.key}
            style={column.width === undefined ? undefined : { width: column.width }}
          />
        ))}
      </colgroup>
      <thead className="sticky top-0 bg-panel">
        <tr className="border-b border-line">
          {columns.map((column) => (
            <SortHead key={column.key} column={column} sort={sort} onSort={setSort} />
          ))}
        </tr>
      </thead>
      <tbody>
        {sortGroups(rows, sort, by).map((row) => (
          <GroupRows
            key={row.key}
            row={row}
            byFrequency={by === "frequency"}
            airtime={airtime}
            span={columns.length}
            open={opened === row.key}
            onToggle={() => setOpened(opened === row.key ? null : row.key)}
          />
        ))}
      </tbody>
    </table>
  );
}

function SortHead({
  column,
  sort,
  onSort,
}: {
  column: Column;
  sort: GroupSort;
  onSort: (sort: GroupSort) => void;
}) {
  const align = column.right ? "text-right" : "";
  if (column.sort === undefined) {
    return <th className={`${TABLE_HEAD} ${align} truncate`}>{column.label}</th>;
  }
  const by = column.sort;
  const active = sort === by;
  return (
    <th
      className={`${TABLE_HEAD} ${align} p-0`}
      aria-sort={active ? (by === "key" ? "ascending" : "descending") : "none"}
    >
      <Button
        type="button"
        className={`block w-full truncate px-2 py-1 ${align} hover:text-ink focus-visible:outline focus-visible:outline-accent ${
          active ? "text-ink" : ""
        }`}
        onClick={() => onSort(by)}
      >
        {column.label}
        {active ? (by === "key" ? " ↑" : " ↓") : ""}
      </Button>
    </th>
  );
}

function GroupRows({
  row,
  byFrequency,
  airtime,
  span,
  open,
  onToggle,
}: {
  row: GroupRow;
  byFrequency: boolean;
  airtime: boolean;
  span: number;
  open: boolean;
  onToggle: () => void;
}) {
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
        <td
          className={`${TABLE_CELL} truncate text-ink ${byFrequency ? "text-right" : ""}`}
          title={row.label}
        >
          {row.label}
        </td>
        <td className={`${TABLE_CELL} truncate text-ink-dim`}>{kindLabel(row.kind)}</td>
        <td className={`${TABLE_CELL} truncate text-right text-ink`}>{row.count}</td>
        {airtime && (
          <td className={`${TABLE_CELL} truncate text-right text-ink-dim`}>
            {formatAirtime(row.airtimeMs)}
          </td>
        )}
        <td
          className={`${TABLE_CELL} truncate text-ink-dim`}
          title={`First ${row.firstAt}\nLast ${row.lastAt}`}
        >
          {formatClock(row.lastAt)}
        </td>
        <td className={`${TABLE_CELL} truncate text-ink`} title={row.latest.summary}>
          {row.latest.summary}
        </td>
      </tr>
      {open && (
        <tr className="border-b border-line/50 bg-panel-2">
          <td colSpan={span} className="px-3 py-2">
            <RowDetail row={row.latest} />
          </td>
        </tr>
      )}
    </>
  );
}
