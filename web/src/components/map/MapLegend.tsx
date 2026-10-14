import type { ReactNode } from "react";
import type { BasemapKind } from "../../lib/map/basemap";
import { HEAT_OPACITY } from "../../lib/map/heat";
import { SIGNAL_GRADIENT } from "../../lib/map/signal";
import { SIGNAL_MAX_DBFS, SIGNAL_MIN_DBFS } from "../../lib/signalSurvey";
import { colormapGradient } from "../plotFrame";

export interface LegendGroup {
  key: string;
  label: string;
  color: string;
  count: number;
}

const ROW = "flex items-center gap-2 font-mono text-2xs tabular-nums";
const BADGE =
  "rounded-control border border-line bg-bg/85 px-2 py-1 font-mono text-2xs text-ink-dim";

function Row({
  swatch,
  label,
  value,
  title,
  danger = false,
}: {
  swatch: ReactNode;
  label: string;
  value?: string | number;
  title?: string;
  danger?: boolean;
}) {
  return (
    <div className={title === undefined ? ROW : `${ROW} pointer-events-auto`} title={title}>
      {swatch}
      <span className={danger ? "text-danger" : "text-ink-dim"}>{label}</span>
      {value !== undefined && (
        <span className={`ml-auto ${danger ? "text-danger" : "text-ink"}`}>{value}</span>
      )}
    </div>
  );
}

function Dot({ color, className }: { color?: string; className?: string }) {
  return (
    <span
      className={`inline-block h-2 w-2 shrink-0 rounded-full ${className ?? ""}`}
      style={color === undefined ? undefined : { backgroundColor: color }}
    />
  );
}

export function MapLegend({
  groups,
  positionCount,
  signalCells,
  heat,
  heatRefused,
  headings,
  basemap,
}: {
  groups: readonly LegendGroup[];
  positionCount: number | null;
  signalCells: number | null;
  heat: boolean;
  heatRefused: string | null;
  headings: boolean;
  basemap: BasemapKind;
}) {
  const hasRows = groups.length > 0 || positionCount !== null || signalCells !== null || heat;
  return (
    <div className="pointer-events-none absolute top-2 left-2 flex flex-col items-start gap-1">
      {hasRows && (
        <div className="flex flex-col gap-1 rounded-control border border-line bg-bg/85 px-2 py-1.5">
          {groups.map((group) => (
            <Row
              key={group.key}
              swatch={<Dot color={group.color} />}
              label={group.label}
              value={group.count}
            />
          ))}
          {positionCount !== null && (
            <Row swatch={<Dot className="bg-accent" />} label="GPS trail" value={positionCount} />
          )}
          {signalCells !== null && <SignalRows cells={signalCells} />}
          {heat && <HeatRow />}
        </div>
      )}
      {basemap === "blank" && <div className={BADGE}>no basemap</div>}
      {!headings && <div className={BADGE}>no headings</div>}
      {heatRefused !== null && (
        <div className={`${BADGE} pointer-events-auto text-danger`} title={heatRefused}>
          no heat
        </div>
      )}
    </div>
  );
}

function SignalRows({ cells }: { cells: number }) {
  return (
    <div className="flex min-w-36 flex-col gap-1 font-mono text-2xs tabular-nums">
      <div className="flex items-center justify-between gap-3">
        <span className="text-ink-dim">Signal cells</span>
        <span className="text-ink">{cells}</span>
      </div>
      <div className="h-1.5 w-full rounded-full" style={{ background: SIGNAL_GRADIENT }} />
      <div className="flex justify-between text-ink-faint">
        <span>{SIGNAL_MIN_DBFS} dBFS</span>
        <span>{SIGNAL_MAX_DBFS} dBFS</span>
      </div>
    </div>
  );
}

function HeatRow() {
  return (
    <Row
      swatch={
        <span
          className="inline-block h-2 w-4 shrink-0 rounded-inset"
          style={{ background: colormapGradient("inferno", "to right"), opacity: HEAT_OPACITY }}
        />
      }
      label="Heat"
    />
  );
}
