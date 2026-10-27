import { ChevronDown, ChevronUp } from "lucide-react";
import type { ReactNode } from "react";
import { useLayoutEffect, useRef, useState } from "react";
import { ISM_KIND_LABELS, percentOf, WIFI_BAND_LABELS } from "../../lib/airtime";
import { copyText } from "../../lib/copyText";
import { useKindLabel } from "../../lib/eventKinds";
import { type GsmCellView, gsmCells, gsmSummary } from "../../lib/gsm";
import {
  berText,
  sidDeviation,
  sidTrends,
  signedDb,
  sparkPoints,
  wholeDb,
} from "../../lib/measurement";
import type { EntityOf } from "../../lib/sinks";
import { toastError } from "../../lib/toasts";
import type { DecodedRecordOf, IdentSignal } from "../../lib/types";
import { useNow } from "../../lib/useNow";
import type { PanelProps, Panels } from "../../sinks/panels";
import { Button } from "../BaseControls";
import { BroadcastDataView } from "../BroadcastDataView";
import { BTN, TABLE_CELL, TABLE_HEAD } from "../controls";
import {
  ageClass,
  buildTranscript,
  formatAge,
  formatClock,
  formatPosition,
  isAtBottom,
  latestRecord,
  latestWpm,
  sortTargets,
  type TargetRow,
  type TargetSort,
  targetRow,
} from "../decoderViews";
import { Readout, Readouts } from "../face/Readouts";
import { formatHz } from "../format";
import { Icon } from "../Icon";
import { Unit } from "../Unit";
import {
  celsius,
  climbRate,
  hectopascal,
  metres,
  metresPerSecond,
  millimetres,
  percent,
  SONDE_LABELS,
  wind,
} from "../weatherFormat";

export type {
  DecodedRecordOf,
  EntityOf,
  GsmCellView,
  IdentSignal,
  PanelProps,
  Panels,
  ReactNode,
  TargetRow,
  TargetSort,
};
export {
  ageClass,
  BroadcastDataView,
  BTN,
  Button,
  berText,
  buildTranscript,
  ChevronDown,
  ChevronUp,
  celsius,
  climbRate,
  copyText,
  formatAge,
  formatClock,
  formatHz,
  formatPosition,
  gsmCells,
  gsmSummary,
  hectopascal,
  Icon,
  ISM_KIND_LABELS,
  isAtBottom,
  latestRecord,
  latestWpm,
  metres,
  metresPerSecond,
  millimetres,
  percent,
  percentOf,
  Readout,
  Readouts,
  SONDE_LABELS,
  sidDeviation,
  sidTrends,
  signedDb,
  sortTargets,
  sparkPoints,
  TABLE_CELL,
  TABLE_HEAD,
  targetRow,
  toastError,
  Unit,
  useKindLabel,
  useNow,
  WIFI_BAND_LABELS,
  wholeDb,
  wind,
};

export const PANE = "flex flex-col gap-2 p-3";
export const EMPTY = "text-sm text-ink-dim";

const TARGET_COLUMNS = {
  adsb: {
    title: "Aircraft",
    idHeader: "ICAO",
    labelHeader: "Callsign",
    primaryHeader: "Altitude",
    secondaryHeader: "Speed / track",
  },
  ais: {
    title: "Ships",
    idHeader: "MMSI",
    labelHeader: "Name",
    primaryHeader: "Speed",
    secondaryHeader: "Course / destination",
  },
} as const;

export function TargetsView({
  entities,
}: {
  entities: readonly (EntityOf<"adsb"> | EntityOf<"ais">)[];
}) {
  const now = useNow(1000);
  const [sort, setSort] = useState<TargetSort>("age");
  const [descending, setDescending] = useState(false);

  const toggle = (key: TargetSort): void => {
    if (key === sort) {
      setDescending(!descending);
    } else {
      setSort(key);
      setDescending(false);
    }
  };

  const rows = entities.map((entity) => targetRow(entity, now));
  const kind = entities[0]?.key.kind === "ais" ? "ais" : "adsb";

  return (
    <div className={PANE}>
      <TargetTable
        {...TARGET_COLUMNS[kind]}
        rows={sortTargets(rows, sort, descending)}
        sort={sort}
        descending={descending}
        onSort={toggle}
      />
    </div>
  );
}

function TargetTable({
  title,
  idHeader,
  labelHeader,
  primaryHeader,
  secondaryHeader,
  rows,
  sort,
  descending,
  onSort,
}: {
  title: string;
  idHeader: string;
  labelHeader: string;
  primaryHeader: string;
  secondaryHeader: string;
  rows: readonly TargetRow[];
  sort: TargetSort;
  descending: boolean;
  onSort: (key: TargetSort) => void;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div className="flex items-baseline gap-2">
        <span className="legend">{title}</span>
        <span className="font-mono text-2xs tabular-nums text-ink-dim">{rows.length}</span>
      </div>
      {rows.length === 0 ? (
        <span className={EMPTY}>No {title.toLowerCase()} heard.</span>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[32rem] border-collapse">
            <thead>
              <tr className="border-b border-line">
                <th className={TABLE_HEAD} scope="col">
                  <SortButton
                    label={idHeader}
                    sorted={sort === "id" ? (descending ? "down" : "up") : null}
                    onClick={() => onSort("id")}
                  />
                </th>
                <th className={TABLE_HEAD} scope="col">
                  {labelHeader}
                </th>
                <th className={TABLE_HEAD} scope="col">
                  {primaryHeader}
                </th>
                <th className={TABLE_HEAD} scope="col">
                  {secondaryHeader}
                </th>
                <th className={TABLE_HEAD} scope="col">
                  Position
                </th>
                <th className={TABLE_HEAD} scope="col">
                  <SortButton
                    label="Age"
                    sorted={sort === "age" ? (descending ? "down" : "up") : null}
                    onClick={() => onSort("age")}
                  />
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.id} className={`border-b border-line/40 ${ageClass(row.ageMs)}`}>
                  <td className={`${TABLE_CELL} font-semibold`}>{row.id}</td>
                  <td className={TABLE_CELL}>{row.label}</td>
                  <td className={TABLE_CELL}>{row.primary}</td>
                  <td className={TABLE_CELL}>{row.secondary || "-"}</td>
                  <td className={TABLE_CELL}>{row.position}</td>
                  <td className={`${TABLE_CELL} text-right`}>{formatAge(row.ageMs)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function SortButton({
  label,
  sorted,
  onClick,
}: {
  label: string;
  sorted: "up" | "down" | null;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      className="inline-flex items-center gap-1 hover:text-accent"
      onClick={onClick}
    >
      {label}
      {sorted !== null && <Icon glyph={sorted === "down" ? ChevronDown : ChevronUp} size={12} />}
    </Button>
  );
}

export function TextView({
  kind,
  records,
}: {
  kind: "rtty" | "morse" | "psk";
  records: PanelProps<"rtty" | "morse" | "psk">["records"];
}) {
  const kindLabel = useKindLabel();
  const text = buildTranscript(records);
  const wpm = kind === "morse" ? latestWpm(records as readonly DecodedRecordOf<"morse">[]) : null;
  const paneRef = useRef<HTMLPreElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = paneRef.current;
    if (el !== null && stick.current) {
      el.scrollTop = el.scrollHeight;
    }
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- new text is what scrolls the pane
  }, [text]);

  const copy = (): void => {
    copyText(text).catch(toastError);
  };

  return (
    <div className={PANE}>
      <div className="flex items-center gap-3">
        <span className="legend">{kindLabel(kind)}</span>
        {wpm !== null && (
          <span className="font-mono text-xs tabular-nums text-ink">
            {wpm.toFixed(0)} <span className="text-ink-dim">WPM</span>
          </span>
        )}
        <Button type="button" className={`${BTN} ml-auto`} disabled={text === ""} onClick={copy}>
          Copy all
        </Button>
      </div>

      <pre
        ref={paneRef}
        tabIndex={0}
        aria-label={`${kind} transcript`}
        className="max-h-72 min-h-32 flex-1 overflow-auto whitespace-pre-wrap break-words rounded-control border border-line bg-panel px-2 py-1.5 font-mono text-xs text-ink"
        onScroll={(e) => {
          stick.current = isAtBottom(e.currentTarget);
        }}
      >
        {text}
      </pre>
    </div>
  );
}
