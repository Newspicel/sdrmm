import { afterEach, describe, expect, it, vi } from "vitest";
import { createDrawLoop, type DrawLoop, zoomSettled } from "./drawLoop";

function harness() {
  const frames = new Map<number, FrameRequestCallback>();
  const sightings: IntersectionObserverCallback[] = [];
  const resizes: ResizeObserverCallback[] = [];
  const windowListeners = new Map<string, EventListener>();
  let next = 0;
  vi.stubGlobal("window", {
    devicePixelRatio: 2,
    addEventListener: (name: string, listener: EventListener) =>
      windowListeners.set(name, listener),
    removeEventListener: (name: string) => windowListeners.delete(name),
  });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++next, callback);
    return next;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: IntersectionObserverCallback) {
        sightings.push(callback);
      }
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        resizes.push(callback);
      }
      observe() {}
      disconnect() {}
    },
  );
  return {
    pending: () => frames.size,
    frame() {
      const due = [...frames.values()];
      frames.clear();
      for (const callback of due) callback(0);
    },
    visible(on: boolean) {
      sightings.at(-1)?.(
        [{ isIntersecting: on } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    },
    resized() {
      resizes.at(-1)?.([], {} as ResizeObserver);
    },
    pixelRatio(ratio: number) {
      (window as { devicePixelRatio: number }).devicePixelRatio = ratio;
      windowListeners.get("resize")?.(new Event("resize"));
    },
  };
}

describe("createDrawLoop", () => {
  const loops: DrawLoop[] = [];
  afterEach(() => {
    for (const loop of loops.splice(0)) loop.dispose();
    vi.unstubAllGlobals();
  });

  function start(paint: () => boolean) {
    const h = harness();
    const loop = createDrawLoop({} as Element, paint);
    loops.push(loop);
    return { h, loop };
  }

  it("waits until on screen, then paints once and goes idle", () => {
    const paint = vi.fn(() => false);
    const { h } = start(paint);
    expect(h.pending()).toBe(0);
    h.visible(true);
    h.frame();
    expect(paint).toHaveBeenCalledOnce();
    expect(h.pending()).toBe(0);
  });

  it("folds many invalidations into one paint", () => {
    const paint = vi.fn(() => false);
    const { h, loop } = start(paint);
    h.visible(true);
    h.frame();
    loop.invalidate();
    loop.invalidate();
    loop.invalidate();
    expect(h.pending()).toBe(1);
    h.frame();
    expect(paint).toHaveBeenCalledTimes(2);
  });

  it("holds changes off screen and paints them on return", () => {
    const paint = vi.fn(() => false);
    const { h, loop } = start(paint);
    h.visible(true);
    h.frame();
    h.visible(false);
    loop.invalidate();
    expect(h.pending()).toBe(0);
    h.visible(true);
    h.frame();
    expect(paint).toHaveBeenCalledTimes(2);
  });

  it("keeps going while a paint animates", () => {
    let frames = 3;
    const paint = vi.fn(() => --frames > 0);
    const { h } = start(paint);
    h.visible(true);
    for (let i = 0; i < 5; i++) h.frame();
    expect(paint).toHaveBeenCalledTimes(3);
  });

  it("repaints on resize, zoom and a new pixel ratio", () => {
    const paint = vi.fn(() => false);
    const { h } = start(paint);
    h.visible(true);
    h.frame();
    h.resized();
    h.frame();
    zoomSettled(1.5);
    h.frame();
    zoomSettled(1.5);
    h.frame();
    h.pixelRatio(1);
    h.frame();
    expect(paint).toHaveBeenCalledTimes(4);
  });

  it("stops for good once disposed", () => {
    const paint = vi.fn(() => false);
    const { h, loop } = start(paint);
    h.visible(true);
    loop.dispose();
    loop.invalidate();
    expect(h.pending()).toBe(0);
    h.frame();
    expect(paint).not.toHaveBeenCalled();
  });
});
