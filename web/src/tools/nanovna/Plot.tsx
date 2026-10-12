import { useId, useRef, useState } from "react";
import { useBoxSize } from "../../lib/useBoxSize";
import type { Tick } from "./axis";

const LEFT = 52;
const RIGHT = 14;
const TOP = 22;
const BOTTOM = 24;
const DRAG_PX = 6;

export interface Domain {
  low: number;
  high: number;
}

export interface PlotTrace {
  key: string;
  label: string;
  color: string;
  dashed: boolean;
  xs: readonly number[];
  values: readonly number[];
}

export interface PlotBand {
  label: string;
  low: number;
  high: number;
}

export interface PlotGuide {
  value: number;
  label: string;
}

export interface PlotMarker {
  id: string;
  label: string;
  x: number;
  active: boolean;
}

export interface Reading {
  title: string;
  rows: { key: string; label: string; color: string; value: string }[];
}

interface Frame {
  width: number;
  height: number;
  x: (value: number) => number;
  y: (value: number) => number;
  invertX: (pixel: number) => number;
}

interface Drag {
  from: number;
  to: number;
}

export function Plot({
  label,
  xDomain,
  yDomain,
  xTicks,
  yTicks,
  traces,
  bands = [],
  guides = [],
  markers,
  readAt,
  onPick,
  onZoom,
  onStep,
}: {
  label: string;
  xDomain: Domain;
  yDomain: Domain;
  xTicks: (domain: Domain, width: number) => Tick[];
  yTicks: (domain: Domain, height: number) => Tick[];
  traces: readonly PlotTrace[];
  bands?: readonly PlotBand[];
  guides?: readonly PlotGuide[];
  markers: readonly PlotMarker[];
  readAt: (x: number) => Reading | null;
  onPick: (x: number) => void;
  onZoom?: (domain: Domain) => void;
  onStep?: (delta: number) => void;
}) {
  const [box, size] = useBoxSize<HTMLDivElement>();
  const [hover, setHover] = useState<number | null>(null);
  const [drag, setDragState] = useState<Drag | null>(null);
  const dragging = useRef<Drag | null>(null);
  const clip = useId();
  const frame = frameFor(size.width, size.height, xDomain, yDomain);

  function setDrag(next: Drag | null) {
    dragging.current = next;
    setDragState(next);
  }

  function begin(event: React.PointerEvent<SVGSVGElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    const pixel = localX(event);
    setDrag({ from: pixel, to: pixel });
  }

  function move(event: React.PointerEvent<SVGSVGElement>) {
    const pixel = localX(event);
    setHover(clamp(pixel, LEFT, frame.width - RIGHT));
    const current = dragging.current;
    if (current !== null) {
      setDrag({ ...current, to: pixel });
    }
  }

  function end() {
    const finished = dragging.current;
    if (finished === null) {
      return;
    }
    const low = Math.min(finished.from, finished.to);
    const high = Math.max(finished.from, finished.to);
    if (high - low >= DRAG_PX && onZoom !== undefined) {
      onZoom({ low: frame.invertX(low), high: frame.invertX(high) });
    } else {
      onPick(frame.invertX(finished.to));
    }
    setDrag(null);
  }

  function key(event: React.KeyboardEvent<SVGSVGElement>) {
    const delta = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0;
    if (delta !== 0 && onStep !== undefined) {
      event.preventDefault();
      onStep(delta * (event.shiftKey ? 10 : 1));
    }
  }

  const reading = hover === null || drag !== null ? null : readAt(frame.invertX(hover));

  return (
    <div ref={box} className="relative h-full min-h-0 w-full min-w-0">
      {frame.width > 0 && (
        <svg
          role="img"
          tabIndex={0}
          data-hotkeys="off"
          aria-label={label}
          width={frame.width}
          height={frame.height}
          className="block touch-none rounded-[3px] bg-plot-bg outline-none select-none focus-visible:ring-2 focus-visible:ring-accent/40"
          onPointerDown={begin}
          onPointerMove={move}
          onPointerUp={end}
          onPointerCancel={() => setDrag(null)}
          onPointerLeave={() => setHover(null)}
          onKeyDown={key}
        >
          <title>{label}</title>
          <defs>
            <clipPath id={clip}>
              <rect
                x={LEFT}
                y={TOP}
                width={frame.width - LEFT - RIGHT}
                height={frame.height - TOP - BOTTOM}
              />
            </clipPath>
          </defs>
          <Bands frame={frame} bands={bands} clip={clip} />
          <Grid
            frame={frame}
            xTicks={xTicks(xDomain, frame.width)}
            yTicks={yTicks(yDomain, frame.height)}
          />
          <Guides frame={frame} guides={guides} />
          <g clipPath={`url(#${clip})`}>
            {traces.map((trace) => (
              <path
                key={trace.key}
                d={pathFor(trace, frame)}
                fill="none"
                stroke={trace.color}
                strokeWidth={trace.dashed ? 1.5 : 2}
                strokeDasharray={trace.dashed ? "6 4" : undefined}
                strokeLinejoin="round"
              />
            ))}
          </g>
          <Markers frame={frame} markers={markers} />
          {drag !== null && Math.abs(drag.to - drag.from) >= DRAG_PX && (
            <rect
              x={Math.min(drag.from, drag.to)}
              y={TOP}
              width={Math.abs(drag.to - drag.from)}
              height={frame.height - TOP - BOTTOM}
              className="fill-plot-ink/10 stroke-plot-ink/40"
            />
          )}
          {reading !== null && hover !== null && (
            <line
              x1={hover}
              x2={hover}
              y1={TOP}
              y2={frame.height - BOTTOM}
              className="stroke-plot-ink/35"
              strokeDasharray="2 3"
            />
          )}
        </svg>
      )}
      {reading !== null && hover !== null && (
        <Tooltip reading={reading} left={hover} width={frame.width} />
      )}
    </div>
  );
}

function localX(event: React.PointerEvent<SVGSVGElement>): number {
  return event.clientX - event.currentTarget.getBoundingClientRect().left;
}

function frameFor(width: number, height: number, xDomain: Domain, yDomain: Domain): Frame {
  const plotWidth = Math.max(1, width - LEFT - RIGHT);
  const plotHeight = Math.max(1, height - TOP - BOTTOM);
  const xSpan = xDomain.high - xDomain.low || 1;
  const ySpan = yDomain.high - yDomain.low || 1;
  return {
    width,
    height,
    x: (value) => LEFT + ((value - xDomain.low) / xSpan) * plotWidth,
    y: (value) =>
      clamp(TOP + ((yDomain.high - value) / ySpan) * plotHeight, TOP - 1, TOP + plotHeight + 1),
    invertX: (pixel) => xDomain.low + ((pixel - LEFT) / plotWidth) * xSpan,
  };
}

function Bands({ frame, bands, clip }: { frame: Frame; bands: readonly PlotBand[]; clip: string }) {
  return (
    <g clipPath={`url(#${clip})`}>
      {bands.map((band) => {
        const left = frame.x(band.low);
        const right = frame.x(band.high);
        return (
          <g key={`${band.label}${band.low}`}>
            <rect
              x={left}
              y={TOP}
              width={Math.max(1, right - left)}
              height={frame.height - TOP - BOTTOM}
              className="fill-band-amateur/20"
            />
            <text
              x={Math.max(left, LEFT) + 4}
              y={TOP + 11}
              className="fill-band-amateur text-[10px] font-medium"
            >
              {band.label}
            </text>
          </g>
        );
      })}
    </g>
  );
}

function Grid({ frame, xTicks, yTicks }: { frame: Frame; xTicks: Tick[]; yTicks: Tick[] }) {
  return (
    <g className="font-mono text-[10px] tabular-nums">
      {yTicks.map((tick) => (
        <g key={`y${tick.value}`}>
          <line
            x1={LEFT}
            x2={frame.width - RIGHT}
            y1={frame.y(tick.value)}
            y2={frame.y(tick.value)}
            stroke="var(--color-plot-grid)"
          />
          <text
            x={LEFT - 6}
            y={frame.y(tick.value) + 3.5}
            textAnchor="end"
            className="fill-plot-ink-dim"
          >
            {tick.label}
          </text>
        </g>
      ))}
      {xTicks.map((tick) => (
        <g key={`x${tick.value}`}>
          <line
            x1={frame.x(tick.value)}
            x2={frame.x(tick.value)}
            y1={TOP}
            y2={frame.height - BOTTOM}
            stroke="var(--color-plot-grid)"
          />
          <text
            x={frame.x(tick.value)}
            y={frame.height - 8}
            textAnchor={anchorFor(frame.x(tick.value), frame.width)}
            className="fill-plot-ink-dim"
          >
            {tick.label}
          </text>
        </g>
      ))}
    </g>
  );
}

function Guides({ frame, guides }: { frame: Frame; guides: readonly PlotGuide[] }) {
  return (
    <g>
      {guides.map((guide) => (
        <g key={guide.label}>
          <line
            x1={LEFT}
            x2={frame.width - RIGHT}
            y1={frame.y(guide.value)}
            y2={frame.y(guide.value)}
            className="stroke-plot-ink/45"
            strokeDasharray="4 4"
          />
          <text
            x={frame.width - RIGHT - 4}
            y={frame.y(guide.value) - 4}
            textAnchor="end"
            className="fill-plot-ink/60 font-mono text-[10px]"
          >
            {guide.label}
          </text>
        </g>
      ))}
    </g>
  );
}

function Markers({ frame, markers }: { frame: Frame; markers: readonly PlotMarker[] }) {
  return (
    <g>
      {markers.map((marker) => {
        const x = frame.x(marker.x);
        if (x < LEFT - 1 || x > frame.width - RIGHT + 1) {
          return null;
        }
        const tone = marker.active ? "stroke-plot-ink" : "stroke-plot-ink/50";
        return (
          <g key={marker.id}>
            <line x1={x} x2={x} y1={TOP} y2={frame.height - BOTTOM} className={tone} />
            <path
              d={`M${x - 7},${TOP - 14} h14 v8 l-7,6 l-7,-6 z`}
              className={marker.active ? "fill-plot-ink" : "fill-plot-ink/60"}
            />
            <text
              x={x}
              y={TOP - 6.5}
              textAnchor="middle"
              className="fill-plot-bg font-mono text-[9px] font-bold"
            >
              {marker.label}
            </text>
          </g>
        );
      })}
    </g>
  );
}

function Tooltip({ reading, left, width }: { reading: Reading; left: number; width: number }) {
  const flip = left > width * 0.62;
  return (
    <div
      className="pointer-events-none absolute top-6 z-10 min-w-36 rounded-[3px] border border-line-strong/70 bg-panel-3 px-2 py-1.5 shadow-pop"
      style={flip ? { right: width - left + 12 } : { left: left + 12 }}
    >
      <div className="mb-1 font-mono text-[11px] text-ink">{reading.title}</div>
      {reading.rows.map((row) => (
        <div key={row.key} className="flex items-center gap-2 font-mono text-[11px] tabular-nums">
          <span className="h-0.5 w-3 shrink-0 rounded-full" style={{ background: row.color }} />
          <span className="text-ink-dim">{row.label}</span>
          <span className="ml-auto pl-3 text-ink">{row.value}</span>
        </div>
      ))}
    </div>
  );
}

function pathFor(trace: PlotTrace, frame: Frame): string {
  const parts: string[] = [];
  let pen = "M";
  trace.values.forEach((value, index) => {
    const x = trace.xs[index];
    if (x === undefined || !Number.isFinite(value)) {
      pen = "M";
      return;
    }
    parts.push(`${pen}${frame.x(x).toFixed(1)},${frame.y(value).toFixed(1)}`);
    pen = "L";
  });
  return parts.join("");
}

function anchorFor(x: number, width: number): "start" | "middle" | "end" {
  if (x > width - RIGHT - 28) {
    return "end";
  }
  return x < LEFT + 28 ? "start" : "middle";
}

export function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}
