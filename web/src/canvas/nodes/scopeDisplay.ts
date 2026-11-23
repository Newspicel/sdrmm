import { type RefObject, useLayoutEffect, useMemo, useRef, useState } from "react";
import { type DbWindow, TRACE_MODES, type TraceMode } from "../../components/spectrumTraces";
import { FULL_VIEW, type SpectrumView } from "../../components/spectrumView";
import {
  AVERAGE_CHOICES,
  type AverageFrames,
  DEFAULT_AVERAGE,
} from "../../components/videoAverage";
import type { Palette, Rgb } from "../../gl/colormap";
import {
  isBoolean,
  isFiniteNumber,
  isRecord,
  listOf,
  oneOf,
  orNull,
  within,
} from "../../lib/remembered";
import { SPECTRUM_OVERLAPS, type SpectrumOverlap } from "../../lib/spectrum";
import { useNodeRemembered } from "../useNodeRemembered";
import {
  type PaletteChoice,
  paletteOf,
  readChoice,
  readCustom,
  storeChoice,
  storeCustom,
} from "./scopePalette";

const AVERAGE_KEY = "sdrmm.scopeAverage";
const OVERLAP_KEY = "sdrmm.scopeOverlap";
const NO_TRACES: readonly TraceMode[] = [];
const TRACE_FRACTION = 0.32;
const IS_TRACES = listOf(oneOf(TRACE_MODES));
const IS_RANGE = orNull(isDbWindow);
const IS_UNIT = within(0, 1);

export function isSpectrumView(value: unknown): value is SpectrumView {
  return isRecord(value) && IS_UNIT(value.start) && IS_UNIT(value.end) && value.start < value.end;
}

export function isDbWindow(value: unknown): value is DbWindow {
  return (
    isRecord(value) &&
    isFiniteNumber(value.min) &&
    isFiniteNumber(value.max) &&
    value.min < value.max
  );
}

export interface ScopeLive {
  view: SpectrumView;
  modes: readonly TraceMode[];
  range: DbWindow | null;
  average: AverageFrames;
}

export interface ScopeDisplay {
  live: RefObject<ScopeLive>;
  view: SpectrumView;
  setView: (next: SpectrumView | ((current: SpectrumView) => SpectrumView)) => void;
  traceModes: readonly TraceMode[];
  toggleTrace: (mode: TraceMode) => void;
  phosphor: boolean;
  setPhosphor: (on: boolean) => void;
  average: AverageFrames;
  chooseAverage: (frames: AverageFrames) => void;
  overlap: SpectrumOverlap;
  chooseOverlap: (overlap: SpectrumOverlap) => void;
  range: DbWindow | null;
  holdRange: (next: DbWindow | null) => void;
  paletteChoice: PaletteChoice;
  chooseColormap: (next: PaletteChoice) => void;
  customStops: readonly Rgb[];
  chooseCustom: (stops: readonly Rgb[]) => void;
  colormap: Palette;
  traceFraction: number;
  setTraceFraction: (fraction: number) => void;
}

export function readAverage(): AverageFrames {
  try {
    const stored = Number(localStorage.getItem(AVERAGE_KEY));
    return AVERAGE_CHOICES.find((frames) => frames === stored) ?? DEFAULT_AVERAGE;
  } catch {
    return DEFAULT_AVERAGE;
  }
}

function storeAverage(frames: AverageFrames): void {
  try {
    localStorage.setItem(AVERAGE_KEY, String(frames));
  } catch {}
}

export function readOverlap(): SpectrumOverlap {
  try {
    const stored = localStorage.getItem(OVERLAP_KEY);
    return SPECTRUM_OVERLAPS.find((overlap) => overlap === stored) ?? "auto";
  } catch {
    return "auto";
  }
}

function storeOverlap(overlap: SpectrumOverlap): void {
  try {
    localStorage.setItem(OVERLAP_KEY, overlap);
  } catch {}
}

export function useScopeDisplay(node: string): ScopeDisplay {
  const [view, setView] = useNodeRemembered(node, "scopeView", FULL_VIEW, isSpectrumView);
  const [traceModes, setTraceModes] = useNodeRemembered(node, "scopeTraces", NO_TRACES, IS_TRACES);
  const [phosphor, setPhosphor] = useNodeRemembered(node, "scopePhosphor", false, isBoolean);
  const [average, setAverage] = useState<AverageFrames>(readAverage);
  const [overlap, setOverlap] = useState<SpectrumOverlap>(readOverlap);
  const [range, setRange] = useNodeRemembered(node, "scopeRange", null, IS_RANGE);
  const [paletteChoice, setPaletteChoice] = useState<PaletteChoice>(readChoice);
  const [customStops, setCustomStops] = useState<readonly Rgb[]>(readCustom);
  const [traceFraction, setTraceFraction] = useNodeRemembered(
    node,
    "scopeTraceFraction",
    TRACE_FRACTION,
    IS_UNIT,
  );
  const live = useRef<ScopeLive>({ view, modes: traceModes, range, average });

  useLayoutEffect(() => {
    live.current = { view, modes: traceModes, range, average };
  });

  return {
    live,
    view,
    setView,
    traceModes,
    toggleTrace: (mode) =>
      setTraceModes((current) =>
        current.includes(mode) ? current.filter((name) => name !== mode) : [...current, mode],
      ),
    phosphor,
    setPhosphor,
    average,
    chooseAverage: (frames) => {
      setAverage(frames);
      storeAverage(frames);
    },
    overlap,
    chooseOverlap: (next) => {
      setOverlap(next);
      storeOverlap(next);
    },
    range,
    holdRange: (next) => {
      live.current = { ...live.current, range: next };
      setRange(next);
    },
    paletteChoice,
    chooseColormap: (next) => {
      setPaletteChoice(next);
      storeChoice(next);
    },
    customStops,
    chooseCustom: (stops) => {
      setCustomStops(stops);
      storeCustom(stops);
    },
    colormap: useMemo(() => paletteOf(paletteChoice, customStops), [paletteChoice, customStops]),
    traceFraction,
    setTraceFraction,
  };
}
