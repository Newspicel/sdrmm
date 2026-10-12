import { useId, useState } from "react";
import { formatHz } from "../../components/format";
import { useBoxSize } from "../../lib/useBoxSize";
import type { PointReadout } from "./analysis";
import type { MarkerRow } from "./markers";
import { formatImpedance } from "./nanovna";
import type { Subject } from "./views";

const PAD = 14;
const RESISTANCES = [0.2, 0.5, 1, 2, 5];
const REACTANCES = [0.2, 0.5, 1, 2, 5];
const SWR_CIRCLE = 2;

interface Geometry {
  center: number;
  radius: number;
  at: (row: PointReadout) => { x: number; y: number };
}

export function SmithChart({
  subjects,
  markers,
  active,
  onPick,
}: {
  subjects: readonly Subject[];
  markers: readonly MarkerRow[];
  active: MarkerRow | null;
  onPick: (hz: number) => void;
}) {
  const [box, size] = useBoxSize<HTMLDivElement>();
  const [hover, setHover] = useState<PointReadout | null>(null);
  const clip = useId();
  const reach = useId();
  const side = Math.max(0, Math.min(size.width, size.height));
  const geometry = geometryFor(side);
  const primary = subjects[0]?.rows ?? [];

  function nearest(event: React.PointerEvent<SVGSVGElement>): PointReadout | null {
    const bounds = event.currentTarget.getBoundingClientRect();
    const u = (event.clientX - bounds.left - geometry.center) / geometry.radius;
    const v = -(event.clientY - bounds.top - geometry.center) / geometry.radius;
    let best: PointReadout | null = null;
    let distance = Number.POSITIVE_INFINITY;
    for (const row of primary) {
      const candidate = Math.hypot(row.s11.re - u, row.s11.im - v);
      if (candidate < distance) {
        best = row;
        distance = candidate;
      }
    }
    return best;
  }

  return (
    <div ref={box} className="relative flex h-full min-h-0 w-full items-center justify-center">
      {side > 0 && (
        <svg
          role="img"
          aria-label="S11 on a Smith chart"
          width={side}
          height={side}
          className="block touch-none rounded-control bg-plot-bg select-none"
          onPointerDown={(event) => {
            const row = nearest(event);
            if (row !== null) {
              onPick(row.frequencyHz);
            }
          }}
          onPointerMove={(event) => setHover(nearest(event))}
          onPointerLeave={() => setHover(null)}
        >
          <title>S11 on a Smith chart</title>
          <SmithGrid geometry={geometry} clip={clip} />
          <clipPath id={reach}>
            <circle cx={geometry.center} cy={geometry.center} r={geometry.radius + PAD / 2} />
          </clipPath>
          <g clipPath={`url(#${reach})`}>
            {subjects.toReversed().map((subject) => (
              <path
                key={subject.key}
                d={smithPath(subject.rows, geometry)}
                fill="none"
                stroke={subject.color}
                strokeWidth={subject.dashed ? 1.5 : 2}
                strokeDasharray={subject.dashed ? "6 4" : undefined}
                strokeLinejoin="round"
              />
            ))}
          </g>
          {markers.map((marker) => (
            <SmithMarker
              key={marker.id}
              marker={marker}
              geometry={geometry}
              active={marker.id === active?.id}
            />
          ))}
        </svg>
      )}
      {hover !== null && (
        <div className="pointer-events-none absolute top-1 left-1 rounded-control bg-panel-3/90 px-1.5 py-0.5 font-mono text-2xs text-ink tabular-nums">
          {formatHz(hover.frequencyHz)} · {formatImpedance(hover.impedance)}
        </div>
      )}
    </div>
  );
}

function geometryFor(side: number): Geometry {
  const center = side / 2;
  const radius = Math.max(1, side / 2 - PAD);
  return {
    center,
    radius,
    at: (row) => ({ x: center + row.s11.re * radius, y: center - row.s11.im * radius }),
  };
}

function SmithGrid({ geometry, clip }: { geometry: Geometry; clip: string }) {
  const { center, radius } = geometry;
  const swrRadius = ((SWR_CIRCLE - 1) / (SWR_CIRCLE + 1)) * radius;
  return (
    <g>
      <defs>
        <clipPath id={clip}>
          <circle cx={center} cy={center} r={radius} />
        </clipPath>
      </defs>
      <circle cx={center} cy={center} r={radius} fill="none" className="stroke-plot-ink-dim" />
      <line
        x1={center - radius}
        x2={center + radius}
        y1={center}
        y2={center}
        stroke="var(--color-plot-grid)"
      />
      <g clipPath={`url(#${clip})`} fill="none" stroke="var(--color-plot-grid)">
        {RESISTANCES.map((r) => (
          <circle
            key={`r${r}`}
            cx={center + (r / (1 + r)) * radius}
            cy={center}
            r={radius / (1 + r)}
          />
        ))}
        {REACTANCES.flatMap((x) =>
          [x, -x].map((value) => (
            <circle
              key={`x${value}`}
              cx={center + radius}
              cy={center - radius / value}
              r={Math.abs(radius / value)}
            />
          )),
        )}
      </g>
      <circle
        cx={center}
        cy={center}
        r={swrRadius}
        fill="none"
        className="stroke-plot-ink/40"
        strokeDasharray="4 4"
      />
      <text
        x={center + swrRadius * 0.72 + 3}
        y={center - swrRadius * 0.72 - 3}
        className="fill-plot-ink/60 font-mono text-3xs"
      >
        2:1
      </text>
      {RESISTANCES.map((r) => (
        <text
          key={`label${r}`}
          x={center + ((r - 1) / (r + 1)) * radius + 2}
          y={center - 3}
          className="fill-plot-ink-dim font-mono text-plot"
        >
          {r * 50}
        </text>
      ))}
    </g>
  );
}

function SmithMarker({
  marker,
  geometry,
  active,
}: {
  marker: MarkerRow;
  geometry: Geometry;
  active: boolean;
}) {
  const point = geometry.at(marker.row);
  return (
    <g>
      <circle
        cx={point.x}
        cy={point.y}
        r={active ? 5 : 4}
        className={active ? "fill-plot-ink" : "fill-plot-ink/60"}
        stroke="var(--color-plot-bg)"
        strokeWidth={2}
      />
      <text
        x={point.x + 7}
        y={point.y - 6}
        className={`font-mono text-3xs font-bold ${active ? "fill-plot-ink" : "fill-plot-ink/60"}`}
      >
        {marker.label}
      </text>
    </g>
  );
}

function smithPath(rows: readonly PointReadout[], geometry: Geometry): string {
  return rows
    .map((row, index) => {
      const point = geometry.at(row);
      return `${index === 0 ? "M" : "L"}${point.x.toFixed(1)},${point.y.toFixed(1)}`;
    })
    .join("");
}
