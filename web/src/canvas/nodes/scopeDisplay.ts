import { type RefObject, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { DbWindow, TraceMode } from "../../components/spectrumTraces";
import { FULL_VIEW, type SpectrumView } from "../../components/spectrumView";
import {
  AVERAGE_CHOICES,
  type AverageFrames,
  DEFAULT_AVERAGE,
} from "../../components/videoAverage";
import type { Palette, Rgb } from "../../gl/colormap";
import {
  type PaletteChoice,
  paletteOf,
  readChoice,
  readCustom,
  storeChoice,
  storeCustom,
} from "./scopePalette";

const AVERAGE_KEY = "sdrmm.scopeAverage";

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

export function useScopeDisplay(): ScopeDisplay {
  const [view, setView] = useState<SpectrumView>(FULL_VIEW);
  const [traceModes, setTraceModes] = useState<readonly TraceMode[]>([]);
  const [phosphor, setPhosphor] = useState(false);
  const [average, setAverage] = useState<AverageFrames>(readAverage);
  const [range, setRange] = useState<DbWindow | null>(null);
  const [paletteChoice, setPaletteChoice] = useState<PaletteChoice>(readChoice);
  const [customStops, setCustomStops] = useState<readonly Rgb[]>(readCustom);
  const [traceFraction, setTraceFraction] = useState(0.32);
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
