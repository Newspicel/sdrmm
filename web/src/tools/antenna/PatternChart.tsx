import { useState } from "react";
import { LABEL } from "../../components/controls";
import type { AntennaPatternCut } from "../../lib/types";
import { useBoxSize } from "../../lib/useBoxSize";

const RANGE_DB = 30;
const RINGS_DB = [10, 20];
const SPOKE_DEG = 30;
const PAD = 22;

export type PatternKind = "azimuth" | "elevation";

interface Frame {
  cx: number;
  cy: number;
  radius: number;
  width: number;
  height: number;
  half: boolean;
}

export function PatternChart({
  title,
  kind,
  cut,
  peakDbi,
  half,
}: {
  title: string;
  kind: PatternKind;
  cut: AntennaPatternCut;
  peakDbi: number;
  half: boolean;
}) {
  const [box, size] = useBoxSize<HTMLDivElement>();
  const [hover, setHover] = useState<number | null>(null);
  const frame = frameFor(size.width, size.height, half);
  const sample = hover === null ? null : sampleAt(cut, hover);

  function pick(event: React.PointerEvent<SVGSVGElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    const dx = event.clientX - bounds.left - frame.cx;
    const dy = frame.cy - (event.clientY - bounds.top);
    const angle = kind === "azimuth" ? Math.atan2(dx, dy) : Math.atan2(dy, dx);
    const degrees = ((angle * 180) / Math.PI + 360) % 360;
    setHover(half && degrees > 180 ? (degrees > 270 ? 0 : 180) : degrees);
  }

  return (
    <section className="flex min-w-0 flex-col gap-1.5">
      <h4 className={LABEL}>{title}</h4>
      <div ref={box} className="relative flex h-[clamp(14rem,34vh,22rem)] w-full justify-center">
        <div className="relative" style={{ width: frame.width }}>
          {frame.radius > 0 && (
            <svg
              role="img"
              aria-label={`${title}, peak ${peakDbi.toFixed(1)} dBi`}
              width={frame.width}
              height={frame.height}
              className="block touch-none rounded-control bg-plot-bg select-none"
              onPointerMove={pick}
              onPointerDown={pick}
              onPointerLeave={() => setHover(null)}
            >
              <Grid frame={frame} kind={kind} peakDbi={peakDbi} />
              <path
                d={tracePath(cut, frame, kind, peakDbi)}
                fill="var(--color-plot-trace)"
                fillOpacity={0.12}
                stroke="var(--color-plot-trace)"
                strokeWidth={2}
                strokeLinejoin="round"
              />
              {sample !== null && hover !== null && (
                <HoverRay frame={frame} kind={kind} angle={hover} db={sample} peakDbi={peakDbi} />
              )}
            </svg>
          )}
          {sample !== null && hover !== null && (
            <div className="pointer-events-none absolute top-1 right-1 rounded-control bg-panel-3/90 px-1.5 py-0.5 font-mono text-2xs text-ink tabular-nums">
              {Math.round(hover)}° · {sample.toFixed(1)} dBi
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

function frameFor(room: number, height: number, half: boolean): Frame {
  if (room <= 0 || height <= 0) {
    return { cx: 0, cy: 0, radius: 0, width: room, height, half };
  }
  if (half) {
    const width = Math.min(room, height * 2);
    const radius = Math.max(0, Math.min(width / 2, height - PAD / 2) - PAD);
    return { cx: width / 2, cy: height - PAD * 0.75, radius, width, height, half };
  }
  const width = Math.min(room, height);
  const radius = Math.max(0, Math.min(width, height) / 2 - PAD);
  return { cx: width / 2, cy: height / 2, radius, width, height, half };
}

function polar(frame: Frame, kind: PatternKind, degrees: number, fraction: number) {
  const angle = (degrees * Math.PI) / 180;
  const r = frame.radius * fraction;
  if (kind === "azimuth") {
    return { x: frame.cx + r * Math.sin(angle), y: frame.cy - r * Math.cos(angle) };
  }
  return { x: frame.cx + r * Math.cos(angle), y: frame.cy - r * Math.sin(angle) };
}

function fractionOf(db: number, peakDbi: number): number {
  return Math.min(1, Math.max(0, (db - peakDbi + RANGE_DB) / RANGE_DB));
}

export function sampleAt(cut: AntennaPatternCut, degrees: number): number | null {
  const count = cut.gain_dbi.length;
  if (count === 0) {
    return null;
  }
  const index = Math.round(degrees / cut.step_deg);
  const wrapped =
    Math.round(360 / cut.step_deg) === count ? ((index % count) + count) % count : index;
  return cut.gain_dbi[Math.min(count - 1, Math.max(0, wrapped))] ?? null;
}

function tracePath(
  cut: AntennaPatternCut,
  frame: Frame,
  kind: PatternKind,
  peakDbi: number,
): string {
  const points = cut.gain_dbi.map((db, index) =>
    polar(frame, kind, index * cut.step_deg, fractionOf(db, peakDbi)),
  );
  const path = points
    .map((point, index) => `${index === 0 ? "M" : "L"}${point.x.toFixed(1)},${point.y.toFixed(1)}`)
    .join("");
  return frame.half ? `${path}L${frame.cx},${frame.cy}Z` : `${path}Z`;
}

function Grid({ frame, kind, peakDbi }: { frame: Frame; kind: PatternKind; peakDbi: number }) {
  const sweep = frame.half ? 180 : 360;
  const spokes = Array.from(
    { length: sweep / SPOKE_DEG + (frame.half ? 1 : 0) },
    (_, index) => index * SPOKE_DEG,
  );
  const rings = [0, ...RINGS_DB].map((drop) => ({
    drop,
    fraction: (RANGE_DB - drop) / RANGE_DB,
  }));
  return (
    <g className="font-mono text-3xs tabular-nums">
      {rings.map((ring) => (
        <path
          key={ring.drop}
          d={arc(frame, ring.fraction)}
          fill="none"
          stroke="var(--color-plot-grid)"
          strokeWidth={ring.drop === 0 ? 1.25 : 1}
        />
      ))}
      {spokes.map((degrees) => {
        const tip = polar(frame, kind, degrees, 1);
        const label = polar(frame, kind, degrees, 1 + 11 / Math.max(frame.radius, 1));
        return (
          <g key={degrees}>
            <line
              x1={frame.cx}
              y1={frame.cy}
              x2={tip.x}
              y2={tip.y}
              stroke="var(--color-plot-grid)"
            />
            <text x={label.x} y={label.y + 3.5} textAnchor="middle" className="fill-plot-ink-dim">
              {degrees}°
            </text>
          </g>
        );
      })}
      {rings
        .filter((ring) => ring.drop > 0)
        .map((ring) => {
          const at = polar(frame, kind, kind === "azimuth" ? 180 : 90, ring.fraction);
          return (
            <text
              key={`label${ring.drop}`}
              x={at.x + 4}
              y={kind === "azimuth" ? at.y - 5 : at.y + 11}
              className="fill-plot-ink-dim"
            >
              −{ring.drop}
            </text>
          );
        })}
      <text x={8} y={16} className="fill-plot-ink">
        {peakDbi.toFixed(1)} dBi
      </text>
    </g>
  );
}

function arc(frame: Frame, fraction: number): string {
  const r = frame.radius * fraction;
  if (frame.half) {
    return `M${frame.cx + r},${frame.cy}A${r},${r} 0 0 0 ${frame.cx - r},${frame.cy}Z`;
  }
  return `M${frame.cx + r},${frame.cy}A${r},${r} 0 1 0 ${frame.cx - r},${frame.cy}A${r},${r} 0 1 0 ${frame.cx + r},${frame.cy}Z`;
}

function HoverRay({
  frame,
  kind,
  angle,
  db,
  peakDbi,
}: {
  frame: Frame;
  kind: PatternKind;
  angle: number;
  db: number;
  peakDbi: number;
}) {
  const edge = polar(frame, kind, angle, 1);
  const point = polar(frame, kind, angle, fractionOf(db, peakDbi));
  return (
    <g>
      <line
        x1={frame.cx}
        y1={frame.cy}
        x2={edge.x}
        y2={edge.y}
        className="stroke-plot-ink/35"
        strokeDasharray="2 3"
      />
      <circle
        cx={point.x}
        cy={point.y}
        r={4}
        className="fill-plot-ink stroke-plot-bg"
        strokeWidth={2}
      />
    </g>
  );
}
