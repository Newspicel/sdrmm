import { afterEach, describe, expect, it, vi } from "vitest";
import { watchOnScreen } from "./onScreen";

function stubObserver() {
  const callbacks: IntersectionObserverCallback[] = [];
  const observed: Element[] = [];
  const disconnects: number[] = [];
  const margins: (string | undefined)[] = [];
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        callbacks.push(callback);
        margins.push(options?.rootMargin);
      }
      observe(element: Element) {
        observed.push(element);
      }
      disconnect() {
        disconnects.push(1);
      }
    },
  );
  return {
    observed,
    margins,
    disconnects,
    report(...visible: boolean[]) {
      callbacks[0]?.(
        visible.map((isIntersecting) => ({ isIntersecting }) as IntersectionObserverEntry),
        {} as IntersectionObserver,
      );
    },
  };
}

describe("watchOnScreen", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("reports the latest entry and prerolls before the element shows", () => {
    const stub = stubObserver();
    const element = {} as Element;
    const seen: boolean[] = [];

    const stop = watchOnScreen(element, (visible) => seen.push(visible));
    stub.report(false, true);
    stub.report(false);

    expect(stub.observed).toEqual([element]);
    expect(stub.margins).toEqual(["128px"]);
    expect(seen).toEqual([true, false]);
    stop();
    expect(stub.disconnects).toHaveLength(1);
  });

  it("draws anyway where the browser has no observer", () => {
    vi.stubGlobal("IntersectionObserver", undefined);
    const seen: boolean[] = [];

    expect(() => watchOnScreen({} as Element, (visible) => seen.push(visible))()).not.toThrow();
    expect(seen).toEqual([true]);
  });
});
