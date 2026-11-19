import { polarPoint, TRUE_MARKS } from "../../components/Rose";
import type { PassPoint, SatelliteLook } from "../../lib/types";

const SIZE = 100;
const CENTRE = SIZE / 2;
const HORIZON = 40;
const RINGS_DEG = [0, 30, 60];
const HORIZON_ONLY = [0];
const LABEL_GAP = 6;

/** Distance from the zenith for an elevation: the horizon on the rim, overhead in the middle. */
export function domeRadius(elevationDeg: number): number {
  const clamped = Math.min(90, Math.max(0, elevationDeg));
  return (HORIZON * (90 - clamped)) / 90;
}

function at(azimuthDeg: number, elevationDeg: number): { x: number; y: number } {
  return polarPoint(azimuthDeg, domeRadius(elevationDeg), CENTRE);
}

/** The pass as a line across the sky, above the horizon only. */
export function passPath(track: readonly PassPoint[]): string {
  return track
    .filter((point) => point.elevation_deg >= 0)
    .map((point, index) => {
      const { x, y } = at(point.azimuth_deg, point.elevation_deg);
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
}

/**
 * The sky seen from the ground station: north up, the horizon on the rim, the zenith in the
 * middle. It draws the pass and where the satellite is now; an empty dome keeps its shape.
 */
export function SkyDome({
  look,
  track,
  visible,
  quiet = false,
}: {
  look: SatelliteLook | null;
  track: readonly PassPoint[];
  visible: boolean;
  /** Only the horizon, leaving the middle to a hint. */
  quiet?: boolean;
}) {
  const path = passPath(track);
  const now =
    look === null || look.elevation_deg < 0 ? null : at(look.azimuth_deg, look.elevation_deg);
  return (
    <svg
      role="img"
      aria-label="Sky view"
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      className="size-28 shrink-0 text-ink-faint"
    >
      {(quiet ? HORIZON_ONLY : RINGS_DEG).map((deg) => (
        <circle
          key={deg}
          cx={CENTRE}
          cy={CENTRE}
          r={domeRadius(deg)}
          fill="none"
          className={deg === 0 ? "stroke-line-strong" : "stroke-line"}
        />
      ))}
      {TRUE_MARKS.map((mark) => {
        const { x, y } = polarPoint(mark.deg, HORIZON + LABEL_GAP, CENTRE);
        return (
          <text
            key={mark.label}
            x={x}
            y={y}
            textAnchor="middle"
            dominantBaseline="central"
            className="fill-ink-dim font-mono text-plot"
          >
            {mark.label}
          </text>
        );
      })}
      {path !== "" && <path d={path} fill="none" className="stroke-accent/60" strokeWidth={1.5} />}
      {now !== null && (
        <circle cx={now.x} cy={now.y} r={3} className={visible ? "fill-accent" : "fill-ink-faint"}>
          <title>Where the satellite is now</title>
        </circle>
      )}
    </svg>
  );
}
