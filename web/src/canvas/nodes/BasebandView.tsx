import { Maximize2, Minimize2, X } from "lucide-react";
import { type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "../../components/BaseControls";
import {
  addConstellation,
  addEye,
  type BasebandGrid,
  clearBasebandGrid,
  createBasebandGrid,
  decayBasebandGrid,
  EYE_COMPONENTS,
  type EyeComponent,
  eyeScale,
  foldRate,
  iqScale,
  planeWidth,
  type SymbolState,
  samplesPerSymbol,
  symbolHistogram,
  symbolPhase,
  symbolStates,
  Trend,
} from "../../components/baseband";
import { ICON_BTN_SM, TAB_BAR, tab } from "../../components/controls";
import { Chips, ChoiceChip, NumberChip, ToggleChip } from "../../components/face/Chips";
import { Readout, Readouts } from "../../components/face/Readouts";
import { Icon } from "../../components/Icon";
import { colormapLut } from "../../components/persistence";
import { dequantize } from "../../components/spectrumTraces";
import { FULL_VIEW } from "../../components/spectrumView";
import type { Palette } from "../../gl/colormap";
import { createDrawLoop, type DrawLoop } from "../../gl/drawLoop";
import type { IqFrame, SpectrumFrame, SymbolFrame } from "../../lib/frame";
import { SPECTRUM_SIGNAL_MARGIN_DB } from "../../lib/limits";
import { iqHub, symbolHub } from "../../lib/portTaps";
import { isBoolean, oneOf, within } from "../../lib/remembered";
import { spectrumHub } from "../../lib/spectrum";
import { token } from "../../lib/tokens";
import { Inspect, InspectSection } from "../inspector";
import { useNodeRemembered } from "../useNodeRemembered";
import { type Measurement, measurements } from "./basebandMeasure";
import { drawPlot, GridBitmap, PLOT_FONT, prepareCanvas } from "./scopePlot";
import {
  drawGraticule,
  drawHistogram,
  drawStates,
  drawTrend,
  type PlotBox,
  type PlotInset,
  plotLabel,
} from "./symbolPlot";

export const BASEBAND_VIEWS = [
  "spectrum",
  "constellation",
  "eye",
  "levels",
  "states",
  "quality",
  "drift",
] as const;
export type BasebandView = (typeof BASEBAND_VIEWS)[number];

const EYE_OPTIONS = [
  { value: "i", label: "I" },
  { value: "q", label: "Q" },
  { value: "frequency", label: "freq" },
] as const satisfies readonly { value: EyeComponent; label: string }[];

const SPECTRUM_BINS = 512;
const GRID = 320;
const MIN_SYMBOL_RATE = 1;
const TREND_POINTS = 240;
const SCATTER_VIEWS: readonly BasebandView[] = ["constellation", "eye"];
const SIGNAL_VIEWS: readonly BasebandView[] = ["spectrum", "constellation", "eye", "levels"];
const SYMBOL_VIEWS: readonly BasebandView[] = ["states", "quality", "drift"];
const SCATTER_PAD = 12;
const PLOT_INSET: PlotInset = { top: 10, bottom: 4 };

const IS_EYE = oneOf(EYE_COMPONENTS);
const IS_SYMBOL_RATE = within(MIN_SYMBOL_RATE, Number.MAX_SAFE_INTEGER);

const PORT = "baseband";
const NO_EYE = "Many carriers, no single eye";

export function BasebandView({
  node,
  colormap,
  view,
  onView,
  onClose,
  expanded = false,
  onExpand,
}: {
  node: string;
  colormap: Palette;
  view: BasebandView;
  onView: (view: BasebandView) => void;
  onClose?: () => void;
  expanded?: boolean;
  onExpand?: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [frame, setFrame] = useState<IqFrame | null>(() => iqHub.latest(node, PORT));
  const frameRef = useRef<IqFrame | null>(frame);
  const [eyeComponent, setEyeComponent] = useNodeRemembered(node, "eye", "frequency", IS_EYE);
  const [symbolRate, setSymbolRate] = useNodeRemembered(node, "symbolRate", 4800, IS_SYMBOL_RATE);
  const [decimate, setDecimate] = useNodeRemembered(node, "decimate", true, isBoolean);

  const [symbols, setSymbols] = useState<SymbolFrame | null>(() => symbolHub.latest(node, PORT));
  const symbolsRef = useRef<SymbolFrame | null>(symbols);
  const [trends] = useState(newTrends);

  const insetRef = useRef<PlotInset>(PLOT_INSET);
  const gridRef = useRef<BasebandGrid | null>(null);
  const scaleRef = useRef(1);
  const bitmapRef = useRef<GridBitmap | null>(null);
  const spectrumRef = useRef<SpectrumFrame | null>(null);
  const dbRef = useRef<Float32Array | null>(null);
  const railRef = useRef<Float32Array | null>(null);
  const fold = foldRate(symbols, symbolRate);
  const settingsRef = useRef({ view, eyeComponent, symbolRate, fold, decimate });
  const colormapRef = useRef(colormap);
  const loopRef = useRef<DrawLoop | null>(null);
  useLayoutEffect(() => {
    settingsRef.current = { view, eyeComponent, symbolRate, fold, decimate };
    colormapRef.current = colormap;
  });

  // biome-ignore lint/correctness/useExhaustiveDependencies: the clear is the effect
  useEffect(() => {
    const grid = gridRef.current;
    if (grid !== null) {
      clearBasebandGrid(grid);
      bitmapRef.current?.invalidate();
    }
    loopRef.current?.invalidate();
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- the settings are what the clear reacts to
  }, [view, eyeComponent, symbolRate, fold, decimate]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the recolour is the effect
  useEffect(() => {
    bitmapRef.current?.invalidate();
    loopRef.current?.invalidate();
    // oxlint-disable-next-line react/exhaustive-effect-dependencies -- a new colormap is what the recolour reacts to
  }, [colormap]);

  useEffect(() => {
    const blocks = { seen: 0 };
    return iqHub.subscribe(node, PORT, (burst) => {
      frameRef.current = burst;
      const {
        view: mode,
        eyeComponent: rail,
        symbolRate: rate,
        fold: eyeRate,
        decimate: sparse,
      } = settingsRef.current;
      if (SCATTER_VIEWS.includes(mode)) {
        const grid = gridRef.current ?? createBasebandGrid(GRID, GRID);
        gridRef.current = grid;
        decayBasebandGrid(grid);
        bitmapRef.current?.invalidate();
        const period = samplesPerSymbol(burst.sampleRate, rate);
        if (mode === "constellation") {
          if (symbolsRef.current === null) {
            const step = sparse ? Math.round(period) : 1;
            scaleRef.current = iqScale(burst.samples);
            addConstellation(
              grid,
              burst.samples,
              scaleRef.current,
              step,
              sparse ? symbolPhase(burst.samples, period) : 0,
            );
          }
        } else if (eyeRate !== null) {
          const eyePeriod = samplesPerSymbol(burst.sampleRate, eyeRate);
          addEye(grid, burst.samples, eyePeriod, rail, eyeScale(burst.samples, rail));
        }
      }
      loopRef.current?.invalidate();
      blocks.seen += 1;
      if (blocks.seen === 1 || blocks.seen % 10 === 0) {
        setFrame(burst);
      }
    });
  }, [node]);

  useEffect(() => {
    const blocks = { seen: 0 };
    clearTrends(trends);
    return symbolHub.subscribe(node, PORT, (block) => {
      symbolsRef.current = block;
      recordTrends(trends, block);
      const { view: mode } = settingsRef.current;
      if (mode === "constellation") {
        const grid = gridRef.current ?? createBasebandGrid(GRID, GRID);
        gridRef.current = grid;
        decayBasebandGrid(grid);
        bitmapRef.current?.invalidate();
        scaleRef.current = referenceScale(block);
        addConstellation(grid, paired(block), scaleRef.current);
      }
      loopRef.current?.invalidate();
      blocks.seen += 1;
      if (blocks.seen === 1 || blocks.seen % 4 === 0) {
        setSymbols(block);
      }
    });
  }, [node, trends]);

  useSpectrumRows(node, view === "spectrum", spectrumRef, loopRef);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) {
      return;
    }
    const scatter: Scatter = { grid: null, scale: 1 };
    const loop = createDrawLoop(canvas, () => {
      scatter.grid = gridRef.current;
      scatter.scale = scaleRef.current;
      draw(
        canvas,
        frameRef.current,
        symbolsRef.current,
        settingsRef.current,
        scatter,
        bitmapRef,
        colormapRef.current,
        spectrumRef,
        dbRef,
        railRef,
        trends,
        insetRef.current,
      );
      return false;
    });
    loopRef.current = loop;
    return () => {
      loop.dispose();
      loopRef.current = null;
    };
  }, [trends]);

  const nyquist = frame === null ? Number.POSITIVE_INFINITY : frame.sampleRate / 2;
  const period = frame === null || fold === null ? null : samplesPerSymbol(frame.sampleRate, fold);

  const shown = symbols === null && SYMBOL_VIEWS.includes(view) ? "spectrum" : view;
  const pickView = (name: BasebandView): void => {
    if (name === shown && onClose !== undefined) {
      onClose();
      return;
    }
    onView(name);
  };
  const hint = waiting(shown, frame, symbols);
  const rows = measurements(shown, frame, symbols, period);

  return (
    <div className="@container flex h-full min-h-0 flex-col overflow-hidden">
      <ChartHeader
        shown={shown}
        symbols={symbols !== null}
        onPick={pickView}
        expanded={expanded}
        onExpand={onExpand}
        onClose={onClose}
      />
      <ViewOptions
        node={node}
        view={shown}
        symbols={symbols !== null}
        eyeComponent={eyeComponent}
        onEyeComponent={setEyeComponent}
        decimate={decimate}
        onDecimate={setDecimate}
        symbolRate={symbolRate}
        onSymbolRate={setSymbolRate}
        nyquist={nyquist}
      />
      <ChartPlot
        canvas={canvasRef}
        hint={hint}
        rows={rows}
        title={shown === "eye" && fold === null ? NO_EYE : undefined}
      />
    </div>
  );
}

function ChartHeader({
  shown,
  symbols,
  onPick,
  expanded,
  onExpand,
  onClose,
}: {
  shown: BasebandView;
  symbols: boolean;
  onPick: (view: BasebandView) => void;
  expanded: boolean;
  onExpand?: () => void;
  onClose?: () => void;
}) {
  return (
    <div data-plot-chrome className={`${TAB_BAR} shrink-0 pr-1`}>
      <div role="group" aria-label="Baseband view" className="flex min-w-0 flex-wrap items-stretch">
        {SIGNAL_VIEWS.map((name) => (
          <ViewTab key={name} name={name} active={shown === name} onPick={onPick} />
        ))}
        {symbols && (
          <>
            <span aria-hidden className="mx-1 my-2 w-px bg-line" />
            {SYMBOL_VIEWS.map((name) => (
              <ViewTab key={name} name={name} active={shown === name} onPick={onPick} />
            ))}
          </>
        )}
      </div>
      {onExpand !== undefined && (
        <Button
          type="button"
          aria-label={expanded ? "Leave full screen" : "Show full screen"}
          aria-pressed={expanded}
          title={expanded ? "Back to the panel" : "Fill the window with this chart"}
          className={`${ICON_BTN_SM} ml-auto self-center text-ink-faint`}
          onClick={onExpand}
        >
          <Icon glyph={expanded ? Minimize2 : Maximize2} size={12} />
        </Button>
      )}
      {onClose !== undefined && (
        <Button
          type="button"
          aria-label="Close chart"
          title="Close chart"
          className={`${ICON_BTN_SM} self-center text-ink-faint ${onExpand === undefined ? "ml-auto" : ""}`}
          onClick={onClose}
        >
          <Icon glyph={X} size={12} />
        </Button>
      )}
    </div>
  );
}

function ChartPlot({
  canvas,
  hint,
  rows,
  title,
}: {
  canvas: RefObject<HTMLCanvasElement | null>;
  hint: string | null;
  rows: readonly Measurement[];
  title?: string;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="relative min-h-0 min-w-0 flex-1 bg-plot-bg" title={title}>
        <canvas ref={canvas} className="absolute inset-0 h-full w-full" />
        {hint !== null && (
          <span className="legend font-mono text-2xs pointer-events-none absolute inset-0 flex items-center justify-center text-plot-ink-dim">
            {hint}
          </span>
        )}
      </div>
      {rows.length > 0 && (
        <Readouts ruled={false} columns="fit" className="shrink-0 border-t border-line bg-panel">
          {rows.map((row) => (
            <Readout key={row.label} label={row.label} title={row.hint}>
              {row.value}
            </Readout>
          ))}
        </Readouts>
      )}
    </div>
  );
}

/** Keeps `latest` on the port's newest spectrum row while `active`, redrawing on each. */
function useSpectrumRows(
  node: string,
  active: boolean,
  latest: RefObject<SpectrumFrame | null>,
  loop: RefObject<DrawLoop | null>,
): void {
  useEffect(() => {
    if (!active) {
      return;
    }
    return spectrumHub.subscribe(
      node,
      PORT,
      (row) => {
        latest.current = row;
        loop.current?.invalidate();
      },
      { bins: SPECTRUM_BINS },
    );
  }, [node, active, latest, loop]);
}

function ViewTab({
  name,
  active,
  onPick,
}: {
  name: BasebandView;
  active: boolean;
  onPick: (view: BasebandView) => void;
}) {
  return (
    <Button
      type="button"
      className={tab(active)}
      aria-pressed={active}
      onClick={() => onPick(name)}
    >
      {name}
    </Button>
  );
}

function ViewOptions({
  node,
  view,
  symbols,
  eyeComponent,
  onEyeComponent,
  decimate,
  onDecimate,
  symbolRate,
  onSymbolRate,
  nyquist,
}: {
  node: string;
  view: BasebandView;
  symbols: boolean;
  eyeComponent: EyeComponent;
  onEyeComponent: (component: EyeComponent) => void;
  decimate: boolean;
  onDecimate: (on: boolean) => void;
  symbolRate: number;
  onSymbolRate: (rate: number) => void;
  nyquist: number;
}) {
  const needsRate =
    !symbols && (view === "eye" || view === "levels" || (view === "constellation" && decimate));
  const decimates = view === "constellation" && !symbols;
  if (view !== "eye" && !decimates && !needsRate) {
    return null;
  }
  return (
    <Inspect node={node}>
      <InspectSection label="View">
        <Chips inset="none">
          {view === "eye" && (
            <ChoiceChip
              label="Trace"
              title="Eye trace"
              value={eyeComponent}
              options={EYE_OPTIONS}
              onChange={onEyeComponent}
            />
          )}
          {decimates && (
            <ToggleChip
              label="Symbols only"
              title="Plot one point per symbol instead of every sample"
              on={decimate}
              onChange={onDecimate}
            />
          )}
          {needsRate && (
            <NumberChip
              label="Symbol rate"
              title="Symbol rate"
              value={symbolRate}
              unit="Bd"
              min={MIN_SYMBOL_RATE}
              max={nyquist}
              step={100}
              onCommit={onSymbolRate}
            />
          )}
        </Chips>
      </InspectSection>
    </Inspect>
  );
}

export function waiting(
  view: BasebandView,
  frame: IqFrame | null,
  block: SymbolFrame | null,
): string | null {
  if (view === "quality" || view === "drift" || view === "states") {
    return block === null ? "This decoder reports no symbols" : null;
  }
  if (frame === null && block === null) {
    return "No burst yet";
  }
  return null;
}

interface Trends {
  mer: Trend;
  margin: Trend;
  drift: Trend;
}

function newTrends(): Trends {
  return {
    mer: new Trend(TREND_POINTS),
    margin: new Trend(TREND_POINTS),
    drift: new Trend(TREND_POINTS),
  };
}

function clearTrends(trends: Trends): void {
  trends.mer.clear();
  trends.margin.clear();
  trends.drift.clear();
}

function recordTrends(trends: Trends, block: SymbolFrame): void {
  trends.mer.push(block.merDb);
  trends.margin.push(block.margin);
  trends.drift.push(block.freqErrorHz);
}

export function paired(block: SymbolFrame): Float32Array {
  if (planeWidth(block) === 2) {
    return block.symbols;
  }
  const out = new Float32Array(block.symbols.length * 2);
  for (let i = 0; i < block.symbols.length; i++) {
    out[i * 2] = block.symbols[i] ?? 0;
  }
  return out;
}

export function referenceScale(block: SymbolFrame): number {
  let peak = 0;
  if (planeWidth(block) === 2) {
    for (let i = 0; i + 1 < block.reference.length; i += 2) {
      peak = Math.max(peak, Math.hypot(block.reference[i] ?? 0, block.reference[i + 1] ?? 0));
    }
  } else {
    for (const level of block.reference) {
      peak = Math.max(peak, Math.abs(level));
    }
  }
  return peak > 0 ? peak * 1.4 : 1;
}

function draw(
  canvas: HTMLCanvasElement,
  frame: IqFrame | null,
  block: SymbolFrame | null,
  settings: { view: BasebandView; eyeComponent: EyeComponent; symbolRate: number },
  scatter: Scatter,
  bitmapRef: { current: GridBitmap | null },
  colormap: Palette,
  spectrumRef: { current: SpectrumFrame | null },
  dbRef: { current: Float32Array | null },
  railRef: { current: Float32Array | null },
  trends: Trends,
  inset: PlotInset,
): void {
  const view = settings.view;
  if (view === "quality") {
    drawTrend(
      canvas,
      [
        { trend: trends.mer, colour: token("plot-trace"), label: "MER dB" },
        { trend: trends.margin, colour: token("plot-hold"), label: "margin" },
      ],
      "per block",
      false,
      inset,
    );
    return;
  }
  if (view === "drift") {
    drawTrend(
      canvas,
      [{ trend: trends.drift, colour: token("plot-trace"), label: "carrier" }],
      "Hz",
      true,
      inset,
    );
    return;
  }
  if (view === "states") {
    drawStates(
      canvas,
      block === null ? [] : statesOf(block),
      block === null || planeWidth(block) === 1,
      inset,
    );
    return;
  }
  if (view === "levels") {
    drawLevels(canvas, frame, block, settings, railRef, inset);
    return;
  }
  if (view === "spectrum") {
    const spectrum = spectrumRef.current;
    if (spectrum === null) {
      return;
    }
    const db = dequantize(spectrum, dbRef.current);
    dbRef.current = db;
    drawPlot(canvas, {
      frame: {
        centerHz: spectrum.centerHz,
        spanHz: spectrum.spanHz,
        signalDb: spectrum.floorDb + SPECTRUM_SIGNAL_MARGIN_DB,
        db,
      },
      view: FULL_VIEW,
      window: { min: spectrum.dbMin, max: spectrum.dbMax },
      traces: [],
      density: null,
      offsetAxis: true,
    });
    return;
  }
  if (frame === null) {
    return;
  }
  drawScatter(canvas, scatter, bitmapRef, colormap, view, settings.eyeComponent);
}

function drawLevels(
  canvas: HTMLCanvasElement,
  frame: IqFrame | null,
  block: SymbolFrame | null,
  settings: { symbolRate: number },
  railRef: { current: Float32Array | null },
  inset: PlotInset,
): void {
  if (block !== null) {
    const scale = referenceScale(block);
    drawHistogram(
      canvas,
      symbolHistogram(block.symbols, planeWidth(block), scale),
      block.reference,
      scale,
      inset,
    );
    return;
  }
  if (frame === null) {
    return;
  }
  const period = samplesPerSymbol(frame.sampleRate, settings.symbolRate);
  const rail = discriminator(
    frame.samples,
    period,
    symbolPhase(frame.samples, period),
    railRef.current,
  );
  railRef.current = rail;
  drawHistogram(canvas, symbolHistogram(rail, 1, 1), NO_REFERENCE, 1, inset);
}

const NO_REFERENCE = new Float32Array(0);

const stateCache = new WeakMap<SymbolFrame, SymbolState[]>();

function statesOf(block: SymbolFrame): SymbolState[] {
  const hit = stateCache.get(block);
  if (hit !== undefined) {
    return hit;
  }
  const states = symbolStates(block);
  stateCache.set(block, states);
  return states;
}

export function discriminator(
  samples: Float32Array,
  period: number,
  offset: number,
  scratch: Float32Array | null = null,
): Float32Array {
  const count = samples.length >> 1;
  const step = Math.max(1, Math.round(period));
  const start = Math.max(1, offset);
  const taps = start >= count ? 0 : Math.ceil((count - start) / step);
  const out = scratch !== null && scratch.length >= taps ? scratch : new Float32Array(taps);
  let tap = 0;
  for (let i = start; i < count; i += step) {
    const re = samples[i * 2] ?? 0;
    const im = samples[i * 2 + 1] ?? 0;
    const pr = samples[i * 2 - 2] ?? 0;
    const pi = samples[i * 2 - 1] ?? 0;
    out[tap] = Math.atan2(im * pr - re * pi, re * pr + im * pi) / Math.PI;
    tap += 1;
  }
  return out.length === taps ? out : out.subarray(0, taps);
}

interface Scatter {
  grid: BasebandGrid | null;
  scale: number;
}

function drawScatter(
  canvas: HTMLCanvasElement,
  { grid, scale }: Scatter,
  bitmapRef: { current: GridBitmap | null },
  colormap: Palette,
  view: BasebandView,
  eyeComponent: EyeComponent,
): void {
  const prepared = prepareCanvas(canvas);
  if (prepared === null || grid === null) {
    return;
  }
  const { ctx, width, height } = prepared;
  ctx.font = PLOT_FONT;
  ctx.lineWidth = 1;
  const inner = { w: width - 2 * SCATTER_PAD, h: height - 2 * SCATTER_PAD };
  if (inner.w <= 0 || inner.h <= 0) {
    return;
  }
  const constellation = view === "constellation";
  const side = Math.min(inner.w, inner.h);
  const box = constellation
    ? { x: (width - side) / 2, y: (height - side) / 2, w: side, h: side }
    : { x: SCATTER_PAD, y: SCATTER_PAD, w: inner.w, h: inner.h };

  drawGraticule(ctx, box, constellation ? 8 : 8, constellation ? 8 : 6);
  if (constellation) {
    ctx.strokeStyle = token("plot-ink-dim");
    ctx.globalAlpha = 0.35;
    ctx.setLineDash([2, 4]);
    ctx.beginPath();
    ctx.arc(box.x + box.w / 2, box.y + box.h / 2, box.w / 2, 0, Math.PI * 2);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }

  const bitmap = bitmapRef.current ?? new GridBitmap(grid.width, grid.height);
  bitmapRef.current = bitmap;
  bitmap.blit(ctx, box, (out) => recolour(grid, colormap, out));

  if (constellation) {
    plotLabel(ctx, "I", box.x + box.w - 5, box.y + box.h / 2 - 6, "right");
    plotLabel(ctx, "Q", box.x + box.w / 2 + 6, box.y + 12);
    drawIqTicks(ctx, box, scale);
  } else {
    plotLabel(
      ctx,
      eyeComponent === "frequency" ? "freq" : eyeComponent.toUpperCase(),
      box.x + 5,
      box.y + 12,
    );
    plotLabel(ctx, "2 symbols", box.x + box.w - 5, box.y + box.h - 5, "right");
  }
}

function drawIqTicks(ctx: CanvasRenderingContext2D, box: PlotBox, scale: number): void {
  const half = scale / 2;
  const bottom = box.y + box.h - 5;
  plotLabel(ctx, tickLabel(-half), box.x + box.w / 4, bottom, "center");
  plotLabel(ctx, tickLabel(half), box.x + (box.w * 3) / 4, bottom, "center");
  plotLabel(ctx, tickLabel(half), box.x + 5, box.y + box.h / 4 + 4);
  plotLabel(ctx, tickLabel(-half), box.x + 5, box.y + (box.h * 3) / 4 + 4);
}

export function tickLabel(value: number): string {
  return Number(value.toPrecision(2)).toString();
}

function recolour(grid: BasebandGrid, colormap: Palette, out: Uint8ClampedArray): void {
  const lut = colormapLut(colormap);
  for (let i = 0; i < grid.cells.length; i++) {
    const value = grid.cells[i] ?? 0;
    const at = i * 4;
    if (value <= 0) {
      out[at + 3] = 0;
      continue;
    }
    const entry = Math.min(255, Math.round(value * 255)) * 3;
    out[at] = lut[entry] ?? 0;
    out[at + 1] = lut[entry + 1] ?? 0;
    out[at + 2] = lut[entry + 2] ?? 0;
    out[at + 3] = Math.min(255, Math.round(40 + value * 215));
  }
}

/** The view tabs alone, for a closed chart: picking one opens it on that view. */
export function BasebandTabs({ onPick }: { onPick: (view: BasebandView) => void }) {
  return (
    <div role="group" aria-label="Baseband view" className={`${TAB_BAR} shrink-0 pr-1`}>
      {SIGNAL_VIEWS.map((name) => (
        <ViewTab key={name} name={name} active={false} onPick={onPick} />
      ))}
    </div>
  );
}
