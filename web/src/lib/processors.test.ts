import { afterEach, describe, expect, it } from "vitest";
import { detection, radarUpdate } from "../test/fixtures";
import { isStale, useProcessorStore } from "./processors";
import type { ServerEvent } from "./types";

function radar(node: string, detections: ReturnType<typeof detection>[]): ServerEvent {
  const marks = detections.map((found) => ({
    x: found.range_km,
    y: found.doppler_hz,
    shape: "dot" as const,
  }));
  return {
    type: "ProcessorUpdate",
    data: {
      node,
      reading: { type: "passive_radar", reading: radarUpdate(detections) },
      view: { surface: { marks } },
    },
  };
}

afterEach(() => useProcessorStore.getState().reset());

describe("useProcessorStore", () => {
  it("replaces the view every report so empty reports clear marks", () => {
    const { observe } = useProcessorStore.getState();
    observe(radar("radar", [detection(12), detection(30)]));
    expect(useProcessorStore.getState().byNode.radar?.view.surface?.marks).toHaveLength(2);
    observe(radar("radar", []));
    expect(useProcessorStore.getState().byNode.radar?.view.surface?.marks).toEqual([]);
  });

  it("learns each processor's pace from the gap between reports", () => {
    const { observe } = useProcessorStore.getState();
    observe(radar("radar", []));
    expect(useProcessorStore.getState().byNode.radar?.periodMs).toBe(0);
    observe(radar("radar", []));
    expect(useProcessorStore.getState().byNode.radar?.periodMs).toBeGreaterThanOrEqual(0);
  });

  it("forgets removed nodes", () => {
    const { observe, forget } = useProcessorStore.getState();
    observe(radar("a", [detection(1)]));
    observe(radar("b", [detection(2)]));
    forget(["a"]);
    expect(useProcessorStore.getState().byNode.a).toBeUndefined();
    expect(useProcessorStore.getState().byNode.b).toBeDefined();
  });

  it("keeps the same state when forgetting a node it never saw", () => {
    const before = useProcessorStore.getState().byNode;
    useProcessorStore.getState().forget(["ghost"]);
    expect(useProcessorStore.getState().byNode).toBe(before);
  });

  it("ignores events that are not processor updates", () => {
    useProcessorStore.getState().observe({ type: "Hello", data: { revision: 1, protocol: 1 } });
    expect(useProcessorStore.getState().byNode).toEqual({});
  });
});

describe("isStale", () => {
  it("calls a reading stale after three periods or two seconds", () => {
    expect(isStale(0, 1_999, 100)).toBe(false);
    expect(isStale(0, 2_001, 100)).toBe(true);
    expect(isStale(0, 2_000, 500)).toBe(false);
    expect(isStale(0, 2_001, 500)).toBe(true);
    expect(isStale(0, 3_000, 1_000)).toBe(false);
    expect(isStale(0, 3_001, 1_000)).toBe(true);
  });
});
