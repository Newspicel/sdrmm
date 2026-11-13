import { onThemeChange } from "../lib/theme";
import { watchOnScreen } from "./onScreen";

/// Asks for one paint on the next animation frame. Calls before that frame fold into it.
export interface DrawLoop {
  invalidate(): void;
  dispose(): void;
}

/// A paint returns true while it animates and wants the next frame as well.
export type Paint = () => boolean;

const repaints = new Set<() => void>();
let unfollow: (() => void) | null = null;
let zoom = 1;
let ratio = 1;

/// Repaints every loop at a new canvas zoom, so plots stay sharp once a zoom settles.
export function zoomSettled(next: number): void {
  if (next === zoom) {
    return;
  }
  zoom = next;
  repaintAll();
}

function repaintAll(): void {
  for (const repaint of repaints) {
    repaint();
  }
}

function followPixelRatio(): () => void {
  ratio = window.devicePixelRatio;
  const changed = (): void => {
    if (window.devicePixelRatio !== ratio) {
      ratio = window.devicePixelRatio;
      repaintAll();
    }
  };
  window.addEventListener("resize", changed);
  return () => window.removeEventListener("resize", changed);
}

/// Paints `element` only after `invalidate`, a resize, a zoom, a theme change or a return on
/// screen. Nothing runs while it is off screen or idle, and a hidden tab gets no frames at all.
export function createDrawLoop(element: Element, paint: Paint): DrawLoop {
  const state = { dirty: true, visible: false, frame: 0, live: true };

  const tick = (): void => {
    state.frame = 0;
    state.dirty = false;
    if (paint()) {
      invalidate();
    }
  };
  const schedule = (): void => {
    if (state.frame === 0 && state.dirty && state.visible && state.live) {
      state.frame = requestAnimationFrame(tick);
    }
  };
  const invalidate = (): void => {
    state.dirty = true;
    schedule();
  };

  const unwatch = watchOnScreen(element, (visible) => {
    state.visible = visible;
    schedule();
  });
  const unrepaint = watchRepaint(element, invalidate);

  return {
    invalidate,
    dispose() {
      state.live = false;
      cancelAnimationFrame(state.frame);
      state.frame = 0;
      unwatch();
      unrepaint();
    },
  };
}

/// Calls `changed` whenever a paint of `element` would come out different without new data:
/// a resize, a canvas zoom, a new pixel ratio or a theme change.
export function watchRepaint(element: Element, changed: () => void): () => void {
  const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(changed);
  resize?.observe(element);
  const repaint = (): void => changed();
  repaints.add(repaint);
  unfollow ??= followPixelRatio();
  const untheme = onThemeChange(repaint);
  return () => {
    resize?.disconnect();
    repaints.delete(repaint);
    if (repaints.size === 0) {
      unfollow?.();
      unfollow = null;
    }
    untheme();
  };
}
