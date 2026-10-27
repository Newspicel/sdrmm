import { type PointerEvent as ReactPointerEvent, type RefObject, useRef, useState } from "react";
import {
  isFullView,
  panView,
  type SpectrumView,
  spanToOffset,
  viewToSpan,
} from "../../components/spectrumView";
import type { SetLevels } from "../../lib/levels";
import type { ChannelInfo } from "../../lib/types";
import type { ScopeChannels } from "./scopeChannels";
import type { ScopeDisplay } from "./scopeDisplay";
import type { FrameMeta } from "./scopeFeed";
import { dragTuneHz, markerAt, markerFrom } from "./scopePick";

const DRAG_SLOP_PX = 4;
const GRAB_PX = 12;
const TUNE_THROTTLE_MS = 150;

interface Gesture {
  pointerX: number;
  at: number;
  view: SpectrumView;
  channel: number | null;
  moved: boolean;
  centerHz: number;
  sentHz: number | null;
  sentAt: number;
}

export interface ScopePreview {
  channel: number;
  offsetHz: number;
}

export interface ScopeGesture {
  panning: boolean;
  hovering: boolean;
  preview: ScopePreview | null;
  pointerFraction: (clientX: number) => number;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLDivElement>) => void;
  hover: (at: number | null) => void;
}

export function onPlotSurface(target: EventTarget | null, plot: HTMLElement): boolean {
  if (!(target instanceof Node) || !plot.contains(target)) {
    return false;
  }
  return !(target instanceof Element) || target.closest("[data-plot-chrome]") === null;
}

export function useScopeGesture(input: {
  plotRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  meta: FrameMeta | null;
  channels: readonly ChannelInfo[];
  levels: SetLevels | undefined;
  display: ScopeDisplay;
  hoverRef: RefObject<number | null>;
  tuning: ScopeChannels;
}): ScopeGesture {
  const { plotRef, active, meta, channels, levels, display, hoverRef, tuning } = input;
  const gestureRef = useRef<Gesture | null>(null);
  const [preview, setPreview] = useState<ScopePreview | null>(null);
  const [panning, setPanning] = useState(false);
  const [hovering, setHovering] = useState(false);
  const spanHz = meta?.spanHz ?? 0;

  const hover = (at: number | null): void => {
    hoverRef.current = at;
    setHovering(at !== null);
  };

  const pointerFraction = (clientX: number): number => {
    const rect = plotRef.current?.getBoundingClientRect();
    return rect === undefined || rect.width === 0 ? 0 : (clientX - rect.left) / rect.width;
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const plot = plotRef.current;
    if (!active || event.button !== 0 || plot === null || meta === null || spanHz <= 0) {
      return;
    }
    if (!onPlotSurface(event.target, plot)) {
      return;
    }
    const rect = plot.getBoundingClientRect();
    const at = pointerFraction(event.clientX);
    const grabbed =
      markerFrom(event.target, channels) ??
      markerAt(channels, levels, display.view, meta.centerHz, spanHz, at, GRAB_PX / rect.width);
    if (grabbed !== null) {
      tuning.selectChannel(grabbed.id);
    }
    gestureRef.current = {
      pointerX: event.clientX,
      at,
      view: display.view,
      channel: grabbed?.id ?? null,
      moved: false,
      centerHz: meta.centerHz,
      sentHz: null,
      sentAt: 0,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const dragChannel = (gesture: Gesture, at: number): void => {
    setPreview({
      channel: gesture.channel ?? 0,
      offsetHz: Math.round(spanToOffset(viewToSpan(gesture.view, at), spanHz)),
    });
  };

  const dragCentre = (gesture: Gesture, clientX: number, width: number): void => {
    const hz = dragTuneHz(
      gesture.centerHz,
      spanHz,
      gesture.view,
      gesture.pointerX - clientX,
      width,
    );
    const now = Date.now();
    if (hz !== gesture.sentHz && now - gesture.sentAt >= TUNE_THROTTLE_MS) {
      gesture.sentHz = hz;
      gesture.sentAt = now;
      tuning.tuneCenter(hz);
    }
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const plot = plotRef.current;
    hover(
      active && plot !== null && onPlotSurface(event.target, plot)
        ? pointerFraction(event.clientX)
        : null,
    );
    const gesture = gestureRef.current;
    if (gesture === null) {
      return;
    }
    if (!gesture.moved && Math.abs(event.clientX - gesture.pointerX) < DRAG_SLOP_PX) {
      return;
    }
    gesture.moved = true;
    if (gesture.channel !== null) {
      if (!tuning.heldChannel(gesture.channel)) {
        dragChannel(gesture, pointerFraction(event.clientX));
      }
      return;
    }
    if (isFullView(gesture.view) && tuning.centerHeld) {
      return;
    }
    const width = plot?.getBoundingClientRect().width || 1;
    setPanning(true);
    if (isFullView(gesture.view)) {
      dragCentre(gesture, event.clientX, width);
      return;
    }
    display.setView(panView(gesture.view, (gesture.pointerX - event.clientX) / width));
  };

  const dropped = (gesture: Gesture, event: ReactPointerEvent<HTMLDivElement>): void => {
    if (gesture.channel !== null && preview !== null && meta !== null) {
      tuning.tuneChannel(gesture.channel, meta.centerHz + preview.offsetHz);
      return;
    }
    if (gesture.channel !== null || !isFullView(gesture.view) || tuning.centerHeld) {
      return;
    }
    if (meta === null) {
      return;
    }
    const width = plotRef.current?.getBoundingClientRect().width || 1;
    const hz = dragTuneHz(
      gesture.centerHz,
      meta.spanHz,
      gesture.view,
      gesture.pointerX - event.clientX,
      width,
    );
    if (hz !== gesture.sentHz) {
      tuning.tuneCenter(hz);
    }
  };

  const onPointerUp = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const gesture = gestureRef.current;
    gestureRef.current = null;
    setPanning(false);
    setPreview(null);
    if (gesture === null) {
      return;
    }
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (gesture.moved) {
      dropped(gesture, event);
      return;
    }
    if (meta === null || gesture.channel !== null) {
      return;
    }
    const offsetHz = Math.round(spanToOffset(viewToSpan(gesture.view, gesture.at), meta.spanHz));
    if (tuning.tunableChannel !== null) {
      tuning.tuneChannel(tuning.tunableChannel, meta.centerHz + offsetHz);
    } else {
      tuning.tuneCenter(meta.centerHz + offsetHz);
    }
  };

  return {
    panning,
    hovering,
    preview,
    pointerFraction,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    hover,
  };
}
