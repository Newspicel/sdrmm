import { type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { FrameTween } from "../../components/frameTween";
import { ReadoutHold } from "../../components/readoutHold";
import {
  alignHistory,
  type FrameKey,
  historyLevels,
  retuneAction,
  rowLevel,
  seedRows,
} from "../../components/spectrumAlign";
import {
  accumulateTraces,
  type DbWindow,
  dequantize,
  frameWindow,
  requantize,
  type TraceMode,
  type TraceState,
  traceOf,
} from "../../components/spectrumTraces";
import { viewWidth } from "../../components/spectrumView";
import { quantizeDb, VideoAverage } from "../../components/videoAverage";
import { watchOnScreen } from "../../gl/onScreen";
import { attachWaterfall, type WaterfallView } from "../../gl/waterfall";
import type { SpectrumFrame } from "../../lib/frame";
import { SPECTRUM_SIGNAL_MARGIN_DB } from "../../lib/limits";
import { SPECTRUM_MAX_BINS, spectrumHub } from "../../lib/spectrum";
import type { DeviceSet } from "../../lib/types";
import type { IqLane } from "../lanes";
import type { ScopeDisplay, ScopeLive } from "./scopeDisplay";
import { DensityLayer, drawPlot, type PlotFrame, type PlotTrace, rollOffShare } from "./scopePlot";

export const SCOPE_PORT = "iq";

const EMPTY_WINDOW: DbWindow = { min: -100, max: -20 };

export interface FrameMeta {
  centerHz: number;
  spanHz: number;
  dbMin: number;
  dbMax: number;
  floorDb: number;
}

export interface FeedState {
  renderer: WaterfallView | null;
  frame: SpectrumFrame | null;
  liveDb: Float32Array | null;
  row: Uint8Array | null;
  key: FrameKey | null;
  traces: TraceState | null;
  clean: number | null;
  rate: number | null;
  reseed: number;
  readonly tween: FrameTween;
  readonly readout: ReadoutHold;
  readonly video: VideoAverage;
}

export interface WaterfallBox {
  top: number;
  height: number;
  width: number;
}

export interface ScopeFeed {
  meta: FrameMeta | null;
  glError: string | null;
  waterfall: WaterfallBox;
  scheduleReseed: () => void;
}

function newFeedState(seed: SpectrumFrame | null): FeedState {
  return {
    renderer: null,
    frame: seed,
    liveDb: null,
    row: null,
    key: null,
    traces: null,
    clean: null,
    rate: null,
    reseed: 0,
    tween: new FrameTween(),
    readout: new ReadoutHold(),
    video: new VideoAverage(),
  };
}

export function metaOf(frame: SpectrumFrame): FrameMeta {
  return {
    centerHz: frame.centerHz,
    spanHz: frame.spanHz,
    dbMin: frame.dbMin,
    dbMax: frame.dbMax,
    floorDb: frame.floorDb,
  };
}

export function displayWindow(meta: FrameMeta | null, held: DbWindow | null): DbWindow {
  if (held !== null) {
    return held;
  }
  return meta === null ? EMPTY_WINDOW : { min: meta.dbMin, max: meta.dbMax };
}

function frameKeyOf(frame: SpectrumFrame): FrameKey {
  return { centerHz: frame.centerHz, spanHz: frame.spanHz, bins: frame.bins.length };
}

function waterfallRow(
  frame: SpectrumFrame,
  shown: Float32Array,
  raw: Float32Array,
  held: DbWindow | null,
  scratch: Uint8Array | null,
): Uint8Array {
  if (shown !== raw) {
    return quantizeDb(shown, held ?? frameWindow(frame), scratch);
  }
  return held === null ? frame.bins : requantize(frame.bins, frameWindow(frame), held, scratch);
}

function plotSource(
  frame: SpectrumFrame | null,
  liveDb: Float32Array | null,
  held: DbWindow | null,
): { frame: PlotFrame | null; window: DbWindow } {
  if (frame === null || liveDb === null) {
    return { frame: null, window: held ?? EMPTY_WINDOW };
  }
  return {
    frame: {
      centerHz: frame.centerHz,
      spanHz: frame.spanHz,
      signalDb: frame.floorDb + SPECTRUM_SIGNAL_MARGIN_DB,
      db: liveDb,
    },
    window: held ?? frameWindow(frame),
  };
}

function overlays(
  traces: TraceState | null,
  modes: readonly TraceMode[],
  frame: PlotFrame | null,
): PlotTrace[] {
  if (traces === null || frame === null) {
    return [];
  }
  const shown: PlotTrace[] = [];
  for (const mode of modes) {
    const db = traceOf(traces, mode);
    if (db.length === frame.db.length) {
      shown.push({ mode, db });
    }
  }
  return shown;
}

function seedHistory(node: string, state: FeedState, held: DbWindow | null): void {
  const past = spectrumHub.history(node, SCOPE_PORT);
  if (past.count === 0) {
    return;
  }
  state.renderer?.seed(
    seedRows(past, state.frame, held),
    past.count,
    past.bins,
    historyLevels(past, held),
  );
}

function spectrumListener(
  node: string,
  state: FeedState,
  live: RefObject<ScopeLive>,
  density: RefObject<DensityLayer | null>,
  onMeta: (meta: FrameMeta) => void,
): (frame: SpectrumFrame) => void {
  let count = 0;
  return (frame) => {
    const action = retuneAction(state.key, frameKeyOf(frame));
    state.key = frameKeyOf(frame);
    state.frame = frame;
    const held = live.current.range;
    const window = held ?? frameWindow(frame);
    state.liveDb = dequantize(frame, state.liveDb);
    if (action.kind !== "none") {
      state.video.reset();
    }
    const db = state.video.apply(state.liveDb, live.current.average);
    if (action.kind === "none") {
      state.tween.push(db, performance.now());
    } else {
      state.tween.jump(db, performance.now());
    }
    let seeded = false;
    if (action.kind !== "none") {
      state.traces = null;
      density.current?.clear();
      if (action.kind === "shift") {
        state.renderer?.shiftRows(action.delta);
      } else {
        const rows = spectrumHub.history(node, SCOPE_PORT);
        if (rows.count > 0) {
          state.renderer?.seed(
            alignHistory(rows, { centerHz: frame.centerHz, spanHz: frame.spanHz }, held),
            rows.count,
            rows.bins,
            historyLevels(rows, held),
          );
          seeded = true;
        }
      }
    }
    if (!seeded) {
      state.row = waterfallRow(frame, db, state.liveDb, held, state.row);
      state.renderer?.pushRow(state.row, rowLevel(window, frame.floorDb, Date.now()));
    }
    state.traces = accumulateTraces(state.traces, db);
    density.current?.add(db, live.current.view, window);
    count += 1;
    if (count === 1 || count % 8 === 0) {
      onMeta(metaOf(frame));
    }
  };
}

export function useScopeFeed(input: {
  node: string;
  set: DeviceSet | null;
  source: IqLane | null;
  active: boolean;
  display: ScopeDisplay;
  traceRef: RefObject<HTMLCanvasElement | null>;
  waterfallRef: RefObject<HTMLCanvasElement | null>;
  hoverRef: RefObject<number | null>;
  densityRef: RefObject<DensityLayer | null>;
}): ScopeFeed {
  const { node, set, source, active, display, traceRef, waterfallRef, hoverRef, densityRef } =
    input;
  const live = display.live;
  const setId = set?.id ?? null;
  const [seedFrame] = useState<SpectrumFrame | null>(() =>
    setId === null ? null : spectrumHub.latest(node, SCOPE_PORT),
  );
  const stateRef = useRef<FeedState>(newFeedState(seedFrame));
  const [meta, setMeta] = useState<FrameMeta | null>(() =>
    seedFrame === null ? null : metaOf(seedFrame),
  );
  const [glError, setGlError] = useState<string | null>(null);
  const [onScreen, setOnScreen] = useState(true);
  const [waterfall, setWaterfall] = useState<WaterfallBox>({ top: 0, height: 0, width: 0 });

  useLayoutEffect(() => {
    const state = stateRef.current;
    state.clean = source?.virtual === undefined ? (set?.capabilities.clean_fraction ?? null) : null;
    state.rate = set?.settings.sample_rate ?? null;
    if (!active) {
      hoverRef.current = null;
    }
  });

  useEffect(() => {
    const state = stateRef.current;
    const canvas = waterfallRef.current;
    if (canvas === null) {
      return;
    }
    let renderer: WaterfallView;
    try {
      renderer = attachWaterfall(canvas, setGlError);
    } catch (error) {
      // oxlint-disable-next-line react/set-state-in-effect -- the WebGL context is the external system this effect attaches, and its refusal has to reach the face
      setGlError(error instanceof Error ? error.message : String(error));
      return;
    }
    state.renderer = renderer;
    return () => {
      if (state.reseed !== 0) {
        cancelAnimationFrame(state.reseed);
        state.reseed = 0;
      }
      renderer.dispose();
      state.renderer = null;
    };
  }, [waterfallRef]);

  useEffect(() => {
    stateRef.current.renderer?.setColormap(display.colormap);
  }, [display.colormap]);

  useEffect(() => {
    stateRef.current.renderer?.setWindow(display.view.start, viewWidth(display.view));
  }, [display.view]);

  useEffect(() => {
    if (setId === null) {
      return;
    }
    const state = stateRef.current;
    seedHistory(node, state, live.current.range);
    state.key = state.frame === null ? null : frameKeyOf(state.frame);
    return spectrumHub.subscribe(
      node,
      SCOPE_PORT,
      spectrumListener(node, state, live, densityRef, setMeta),
      { bins: SPECTRUM_MAX_BINS },
    );
  }, [setId, node, live, densityRef]);

  useEffect(() => {
    const plot = traceRef.current;
    return plot === null ? undefined : watchOnScreen(plot, setOnScreen);
  }, [traceRef]);

  useEffect(() => {
    if (!onScreen) {
      return;
    }
    const state = stateRef.current;
    let raf = 0;
    const loop = () => {
      const { frame, window } = plotSource(
        state.frame,
        state.liveDb === null ? null : state.tween.sample(performance.now()),
        live.current.range,
      );
      drawPlot(traceRef.current, {
        frame,
        view: live.current.view,
        window,
        traces: overlays(state.traces, live.current.modes, frame),
        density: densityRef.current,
        cursor: hoverRef.current,
        readout: state.readout,
        rollOff: frame === null ? 0 : rollOffShare(state.clean, state.rate, frame.spanHz),
      });
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [onScreen, live, traceRef, hoverRef, densityRef]);

  useEffect(() => {
    if (!display.phosphor) {
      densityRef.current = null;
      return;
    }
    const layer = new DensityLayer(display.colormap);
    densityRef.current = layer;
    return () => {
      if (densityRef.current === layer) {
        densityRef.current = null;
      }
    };
  }, [densityRef, display.phosphor, display.colormap]);

  useEffect(() => {
    const canvas = waterfallRef.current;
    if (canvas === null) {
      return;
    }
    const measure = () =>
      setWaterfall({
        top: canvas.offsetTop,
        height: canvas.clientHeight,
        width: canvas.clientWidth,
      });
    const observer = new ResizeObserver(measure);
    observer.observe(canvas);
    measure();
    return () => observer.disconnect();
  }, [waterfallRef]);

  return {
    meta,
    glError,
    waterfall,
    scheduleReseed: () => {
      const state = stateRef.current;
      if (state.reseed !== 0 || setId === null) {
        return;
      }
      state.reseed = requestAnimationFrame(() => {
        state.reseed = 0;
        seedHistory(node, state, live.current.range);
      });
    },
  };
}
