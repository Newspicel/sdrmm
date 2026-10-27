import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { attachWaterfall, type WaterfallView } from "./waterfall";

function canvas() {
  const context = { drawImage: vi.fn(), clearRect: vi.fn() };
  const element = {
    width: 300,
    height: 150,
    clientWidth: 320,
    clientHeight: 120,
    getContext: () => context,
    getBoundingClientRect: () => ({ width: element.clientWidth }),
  };
  return { element: element as unknown as HTMLCanvasElement, context };
}

function harness() {
  const draw = vi.fn();
  const uniform3fv = vi.fn();
  const texSubImage2D = vi.fn();
  const callbacks = new Map<number, FrameRequestCallback>();
  const observers: IntersectionObserverCallback[] = [];
  const listeners = new Map<string, EventListener>();
  let next = 0;
  const gl = new Proxy(
    { drawArrays: draw, uniform3fv, texSubImage2D },
    {
      get(target, name) {
        if (name === "drawArrays") return target.drawArrays;
        if (name === "uniform3fv") return target.uniform3fv;
        if (name === "texSubImage2D") return target.texSubImage2D;
        if (name === "getExtension") return () => null;
        if (String(name).toUpperCase() === name) return 1;
        return () => ({});
      },
    },
  );
  const shared = {
    width: 300,
    height: 150,
    getContext: () => gl,
    addEventListener: (name: string, listener: EventListener) => listeners.set(name, listener),
  };
  vi.stubGlobal("window", { devicePixelRatio: 2, setTimeout, clearTimeout });
  vi.stubGlobal("document", { createElement: () => shared });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callbacks.set(++next, callback);
    return next;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => callbacks.delete(id));
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: IntersectionObserverCallback) {
        observers.push(callback);
      }
      observe() {}
      disconnect() {}
    },
  );
  return {
    draw,
    uniform3fv,
    texSubImage2D,
    canvas,
    frame() {
      const pending = [...callbacks.values()];
      callbacks.clear();
      for (const callback of pending) callback(0);
    },
    visible(index: number, visible: boolean) {
      observers[index]?.(
        [{ isIntersecting: visible } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    },
    event(name: string) {
      listeners.get(name)?.({ target: shared, preventDefault() {} } as unknown as Event);
    },
  };
}

describe("waterfall repainting", () => {
  const views: WaterfallView[] = [];
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    for (const view of views.splice(0)) view.dispose();
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("paints new data and changed settings once", () => {
    const h = harness();
    const view = attachWaterfall(h.canvas().element);
    views.push(view);
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(0);
    h.visible(0, true);
    h.frame();
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(1);
    view.pushRow(new Uint8Array(8));
    view.pushRow(new Uint8Array(8));
    h.frame();
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(2);
    view.setWindow(0, 1);
    view.setColormap("classic");
    view.shiftRows(0);
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(2);
    view.setWindow(0.1, 0.8);
    view.setColormap("viridis");
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(3);
    view.shiftRows(0.1);
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(4);
    view.seed(new Uint8Array(16), 2, 8);
    h.frame();
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(5);
  });

  it("stores a row's level in place, without a scratch array per row", () => {
    const h = harness();
    const view = attachWaterfall(h.canvas().element);
    views.push(view);
    h.visible(0, true);
    view.pushRow(new Uint8Array(8), { signal: -40, spanDb: 60, at: 5 });

    const set = vi.spyOn(Float32Array.prototype, "set");
    view.pushRow(new Uint8Array(8), { signal: -30, spanDb: 50, at: 6 });
    expect(set).not.toHaveBeenCalled();
    set.mockRestore();

    h.frame();
    const levels = h.texSubImage2D.mock.calls
      .map((call) => call[8])
      .findLast((data) => data instanceof Float32Array) as Float32Array;
    expect([...levels.subarray(0, 8)]).toEqual([0, -40, 60, 0, 0, -30, 50, 0]);
  });

  it("keeps a custom palette set before the first paint", () => {
    const h = harness();
    const view = attachWaterfall(h.canvas().element);
    views.push(view);
    const red = [1, 0, 0] as const;
    view.setColormap([red, red, red, red, red]);
    h.visible(0, true);
    h.frame();
    expect(Array.from(h.uniform3fv.mock.lastCall?.[1] ?? [])).toEqual([
      1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0,
    ]);
  });

  it("keeps unchanged plots intact when another plot resizes", () => {
    const h = harness();
    const first = h.canvas();
    const second = h.canvas();
    views.push(attachWaterfall(first.element), attachWaterfall(second.element));
    h.visible(0, true);
    h.visible(1, true);
    h.frame();
    Object.defineProperty(second.element, "clientWidth", { value: 1000 });
    h.frame();
    h.frame();
    expect(first.context.drawImage).toHaveBeenCalledTimes(1);
    expect(second.context.drawImage).toHaveBeenCalledTimes(2);
    window.devicePixelRatio = 1;
    h.frame();
    expect(first.context.drawImage).toHaveBeenCalledTimes(2);
    expect(second.context.drawImage).toHaveBeenCalledTimes(3);
  });

  it("retains updates while hidden and redraws on return", () => {
    const h = harness();
    const view = attachWaterfall(h.canvas().element);
    views.push(view);
    h.visible(0, true);
    h.frame();
    h.visible(0, false);
    view.pushRow(new Uint8Array(8));
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(1);
    h.visible(0, true);
    h.frame();
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(2);
  });

  it("paints after context restoration and reports recovery", () => {
    const h = harness();
    const status = vi.fn();
    views.push(attachWaterfall(h.canvas().element, status));
    h.visible(0, true);
    h.frame();
    h.event("webglcontextlost");
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(1);
    expect(status).toHaveBeenLastCalledWith(expect.stringContaining("context lost"));
    h.event("webglcontextrestored");
    h.frame();
    h.frame();
    expect(h.draw).toHaveBeenCalledTimes(2);
    expect(status).toHaveBeenLastCalledWith(null);
  });
});
