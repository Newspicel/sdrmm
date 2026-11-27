import {
  type PointerEvent as ReactPointerEvent,
  type RefObject,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  type FingerPair,
  isFullView,
  panView,
  pinchView,
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
const HOLD_MS = 500;

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

interface Pinch {
  view: SpectrumView;
  from: FingerPair;
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

/// The click a browser sends after a held finger lifts would close the menu the hold opened.
function swallowNextClick(target: Window): void {
  const swallow = (event: MouseEvent): void => {
    event.preventDefault();
    event.stopPropagation();
    forget();
  };
  const forget = (): void => {
    target.removeEventListener("click", swallow, { capture: true });
    target.removeEventListener("pointerdown", forget, { capture: true });
  };
  target.addEventListener("click", swallow, { capture: true });
  target.addEventListener("pointerdown", forget, { capture: true });
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
  onHold: (clientX: number, clientY: number) => void;
}): ScopeGesture {
  const { plotRef, active, meta, channels, levels, display, hoverRef, tuning, onHold } = input;
  const gestureRef = useRef<Gesture | null>(null);
  const touches = useRef(new Map<number, number>());
  const pinchRef = useRef<Pinch | null>(null);
  const holdRef = useRef<number | null>(null);
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

  const cancelHold = (): void => {
    if (holdRef.current !== null) {
      window.clearTimeout(holdRef.current);
      holdRef.current = null;
    }
  };

  useEffect(
    () => () => {
      if (holdRef.current !== null) {
        window.clearTimeout(holdRef.current);
      }
    },
    [],
  );

  /// A finger held still opens the pick menu, since touch has no right click.
  const armHold = (event: ReactPointerEvent<HTMLDivElement>): void => {
    if (event.pointerType !== "touch") {
      return;
    }
    const { clientX, clientY } = event;
    holdRef.current = window.setTimeout(() => {
      holdRef.current = null;
      gestureRef.current = null;
      swallowNextClick(window);
      onHold(clientX, clientY);
    }, HOLD_MS);
  };

  const fingers = (): FingerPair | null => {
    const [a, b] = [...touches.current.values()];
    return a === undefined || b === undefined
      ? null
      : { a: pointerFraction(a), b: pointerFraction(b) };
  };

  const touchDown = (event: ReactPointerEvent<HTMLDivElement>): boolean => {
    if (event.pointerType !== "touch") {
      return false;
    }
    touches.current.set(event.pointerId, event.clientX);
    const from = fingers();
    if (from === null) {
      return false;
    }
    gestureRef.current = null;
    cancelHold();
    setPanning(false);
    setPreview(null);
    pinchRef.current ??= { view: display.view, from };
    event.currentTarget.setPointerCapture(event.pointerId);
    return true;
  };

  const touchMove = (event: ReactPointerEvent<HTMLDivElement>): boolean => {
    if (!touches.current.has(event.pointerId)) {
      return false;
    }
    touches.current.set(event.pointerId, event.clientX);
    const pinch = pinchRef.current;
    const to = fingers();
    if (pinch === null || to === null) {
      return false;
    }
    display.setView(pinchView(pinch.view, pinch.from, to));
    return true;
  };

  const touchUp = (event: ReactPointerEvent<HTMLDivElement>): boolean => {
    touches.current.delete(event.pointerId);
    if (pinchRef.current === null) {
      return false;
    }
    if (touches.current.size < 2) {
      pinchRef.current = null;
    }
    return true;
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>): void => {
    const plot = plotRef.current;
    if (!active || event.button !== 0 || plot === null || meta === null || spanHz <= 0) {
      return;
    }
    if (!onPlotSurface(event.target, plot) || touchDown(event)) {
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
    if (grabbed === null) {
      armHold(event);
    }
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
    if (touchMove(event) || gesture === null) {
      return;
    }
    if (!gesture.moved && Math.abs(event.clientX - gesture.pointerX) < DRAG_SLOP_PX) {
      return;
    }
    gesture.moved = true;
    cancelHold();
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
    cancelHold();
    if (touchUp(event)) {
      return;
    }
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
