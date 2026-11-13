import { MeterBar } from "../../../components/face/Meter";
import type { PatternView } from "../../../lib/types";
import {
  amplitudePercent,
  angleGap,
  NULL_CLOSE_DEG,
  patternPath,
  polarTick,
  weightTitle,
} from "./pattern";

const PATTERN_PX = 120;

export function BeamPattern({ view, dim }: { view: PatternView | null; dim: boolean }) {
  return (
    <div className={`flex gap-2 p-2 ${dim ? "opacity-45" : ""}`}>
      <PatternPlot view={view} />
      <div className="min-w-0 flex-1">
        <WeightBars view={view} />
      </div>
    </div>
  );
}

function PatternPlot({ view }: { view: PatternView | null }) {
  const centre = PATTERN_PX / 2;
  const radius = centre - 6;
  const pattern = view?.pattern ?? [];
  const steer = view?.steer_deg ?? null;
  return (
    <svg
      role="img"
      aria-label="Beam pattern"
      width={PATTERN_PX}
      height={PATTERN_PX}
      viewBox={`0 0 ${PATTERN_PX} ${PATTERN_PX}`}
      className="shrink-0"
    >
      <circle cx={centre} cy={centre} r={radius} className="fill-well stroke-line" />
      <circle cx={centre} cy={centre} r={radius / 2} className="fill-none stroke-line/60" />
      <text x={centre} y={9} textAnchor="middle" className="fill-ink-faint font-mono text-plot">
        Fwd
      </text>
      {pattern.length > 0 && (
        <path d={patternPath(pattern, radius, centre)} className="fill-accent/25 stroke-accent" />
      )}
      {steer !== null && (
        <path
          d={polarTick(steer, radius - 8, radius + 4, centre)}
          className="stroke-accent"
          strokeWidth={2}
        />
      )}
      {(view?.nulls_deg ?? []).map((deg) => (
        <path
          key={deg}
          d={polarTick(deg, radius - 6, radius + 4, centre)}
          className={
            steer !== null && angleGap(deg, steer) < NULL_CLOSE_DEG
              ? "stroke-warn"
              : "stroke-danger"
          }
          strokeWidth={1.5}
        />
      ))}
    </svg>
  );
}

function WeightBars({ view }: { view: PatternView | null }) {
  const weights = view?.weights ?? [];
  if (weights.length === 0) {
    return null;
  }
  const percent = amplitudePercent(weights);
  return (
    <div role="group" className="flex flex-col gap-0.5" aria-label="Lane weights">
      {weights.map((weight, lane) => (
        <div key={lane} className="flex items-center gap-1.5" title={weightTitle(lane, weight)}>
          <span className="w-4 font-mono text-2xs text-hue-indigo">{lane + 1}</span>
          <MeterBar
            label={`Lane ${lane + 1} weight`}
            value={(percent[lane] ?? 0) / 100}
            valueText={weightTitle(lane, weight)}
          />
        </div>
      ))}
    </div>
  );
}
