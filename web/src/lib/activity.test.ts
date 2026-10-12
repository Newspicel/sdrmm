import { beforeEach, describe, expect, it } from "vitest";
import { activityText, idlePort, useActivityStore } from "./activity";
import type { PortActivity } from "./types";

const port = (counts: Partial<PortActivity>): PortActivity => ({
  node: "adsb",
  port: "events",
  emitted: 0,
  delivered: 0,
  unrouted: 0,
  ignored: 0,
  lost: 0,
  ...counts,
});

describe("port activity", () => {
  beforeEach(() => useActivityStore.getState().reset());

  it("keeps the latest counts per node and port", () => {
    useActivityStore.getState().observe({
      type: "NodeActivity",
      data: { ports: [port({ emitted: 3, delivered: 2, ignored: 1 })] },
    });

    expect(useActivityStore.getState().ports["adsb\u0000events"]?.delivered).toBe(2);
  });

  it("names only the counts that happened", () => {
    expect(activityText(port({ emitted: 3, delivered: 2, ignored: 1 }))).toBe(
      "2 delivered, 1 ignored",
    );
    expect(activityText(port({}))).toBeNull();
    expect(activityText(undefined)).toBeNull();
  });

  it("dims a port whose records reach nothing", () => {
    expect(idlePort(port({ emitted: 4, unrouted: 4 }))).toBe(true);
    expect(idlePort(port({ emitted: 4, delivered: 1, unrouted: 3 }))).toBe(false);
  });
});
