import { useState } from "react";
import { GridCanvas } from "../../../components/GridCanvas";
import { ColourScale, PlotAxis } from "../../../components/PlotAxis";
import { linearTicks, type PlotRect, plotRect } from "../../../components/plotFrame";
import { type Colormap, gridCellAt } from "../../../gl/surface";
import { recordEvent } from "../../../lib/diagnostics";
import type { RangeDopplerFrame } from "../../../lib/frame";
import { LIGHT_SPEED_M_S, RADAR_LIMITS } from "../../../lib/limits";
import type { ViewMark } from "../../../lib/types";
import { useBoxSize } from "../../../lib/useBoxSize";
import { useSurface } from "../../../lib/useSurface";
import { SurfaceRefused } from "../SurfaceRefused";
import { MarkLayer } from "./MarkLayer";
import {
  dopplerSpanHz,
  frameAxes,
  hoverText,
  levelDb,
  type PlotAxes,
  pxToSurface,
  rangeSpanKm,
  surfaceToPx,
  velocityMps,
} from "./rangeDoppler";

const GUTTERS = { left: 36, right: 40, top: 16, bottom: 18 };
const TICKS = 5;

export function RangeDopplerPlot({
  node,
  known,
  marks,
  dim,
  colormap,
  picked,
}: {
  node: string;
  known: boolean;
  marks: readonly ViewMark[];
  dim: boolean;
  colormap: Colormap;
  picked: number | null;
}) {
  const [frame, setFrame] = useState<RangeDopplerFrame | null>(null);
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null);
  const [ref, size] = useBoxSize<HTMLDivElement>();
  const refused = useSurface(known ? node : null, (surface) => {
    if (surface.kind === "range_doppler") {
      setFrame(surface.frame);
    } else {
      recordEvent("warn", "surface", `unexpected ${surface.kind} surface`);
    }
  });
  const axes = frame === null ? null : frameAxes(frame);
  const plot = plotRect(size.width, size.height, GUTTERS);
  const grid =
    frame === null ? null : { cols: frame.ranges, rows: frame.dopplers, cells: frame.cells };
  return (
    <div ref={ref} className="relative min-h-48 flex-1 bg-plot-bg">
      <div className={`absolute ${dim ? "opacity-45" : ""}`} style={GUTTERS}>
        <GridCanvas frame={grid} colormap={colormap} flipY />
      </div>
      <span className="absolute top-0.5 left-1 font-mono text-plot text-ink-faint">Hz</span>
      <span className="absolute top-0.5 right-1 font-mono text-plot text-ink-faint">m/s</span>
      <span className="absolute right-1 bottom-0.5 font-mono text-plot text-ink-faint">km</span>
      <span className="absolute top-0.5 left-1/2 -translate-x-1/2">
        <ColourScale
          colormap={colormap}
          min={axes?.dbMin ?? RADAR_LIMITS.surface_db.min}
          max={axes?.dbMax ?? RADAR_LIMITS.surface_db.max}
          unit="dB"
        />
      </span>
      {axes !== null && plot.w > 0 && plot.h > 0 && (
        <svg
          aria-hidden
          className="pointer-events-none absolute inset-0 size-full overflow-visible"
        >
          <RangeDopplerAxes axes={axes} plot={plot} />
          <svg x={plot.x} y={plot.y} width={plot.w} height={plot.h}>
            <MarkLayer
              marks={marks}
              at={(x, y) =>
                surfaceToPx({ rangeKm: x, dopplerHz: y }, axes, { w: plot.w, h: plot.h })
              }
              picked={picked}
            />
          </svg>
        </svg>
      )}
      <SurfaceRefused text={refused} />
      {axes !== null && (
        <div
          className="absolute"
          style={GUTTERS}
          onPointerMove={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            setHover({ x: event.clientX - box.left, y: event.clientY - box.top });
          }}
          onPointerLeave={() => setHover(null)}
        >
          {hover !== null && (
            <span className="pointer-events-none absolute top-1 left-1 rounded-inset bg-bg/85 px-1 font-mono text-2xs tabular-nums text-ink">
              {hoverLabel(hover, axes, plot, frame)}
            </span>
          )}
        </div>
      )}
    </div>
  );
}

function hoverLabel(
  hover: { x: number; y: number },
  axes: PlotAxes,
  plot: PlotRect,
  frame: RangeDopplerFrame | null,
): string {
  const box = { w: plot.w, h: plot.h };
  const point = pxToSurface(hover.x, hover.y, axes, box);
  const cell =
    frame === null
      ? null
      : gridCellAt(hover.x, hover.y, box, { cols: frame.ranges, rows: frame.dopplers }, true);
  const level =
    cell === null || frame === null ? undefined : frame.cells[cell.row * frame.ranges + cell.col];
  return hoverText(point, axes.carrierHz, level === undefined ? null : levelDb(level, axes));
}

function RangeDopplerAxes({ axes, plot }: { axes: PlotAxes; plot: PlotRect }) {
  const box = { w: plot.w, h: plot.h };
  const range = rangeSpanKm(axes);
  const doppler = dopplerSpanHz(axes);
  const rangeTicks = linearTicks(
    range.lo,
    range.hi,
    TICKS,
    (km) => surfaceToPx({ rangeKm: km, dopplerHz: 0 }, axes, box).x,
  );
  const dopplerTicks = linearTicks(
    doppler.lo,
    doppler.hi,
    TICKS,
    (hz) => surfaceToPx({ rangeKm: 0, dopplerHz: hz }, axes, box).y,
  );
  const fast = velocityMps(doppler.lo, axes.carrierHz);
  const slow = velocityMps(doppler.hi, axes.carrierHz);
  const speedTicks =
    fast === null || slow === null
      ? []
      : linearTicks(
          slow,
          fast,
          TICKS,
          (mps) =>
            surfaceToPx(
              { rangeKm: 0, dopplerHz: (-mps * axes.carrierHz) / LIGHT_SPEED_M_S },
              axes,
              box,
            ).y,
        );
  return (
    <>
      <rect
        x={plot.x}
        y={plot.y}
        width={plot.w}
        height={plot.h}
        className="fill-none stroke-line"
      />
      <PlotAxis side="bottom" ticks={rangeTicks} plot={plot} />
      <PlotAxis side="left" ticks={dopplerTicks} plot={plot} />
      <PlotAxis side="right" ticks={speedTicks} plot={plot} />
    </>
  );
}
