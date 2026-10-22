import { describe, expect, it } from "vitest";
import type { DeviceSet, PatchGraph, TimeMachineStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { deviceSet, placed } from "../../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import { TimeMachineFace } from "./TimeMachineFace";

const NODE = placed("history", { kind: "time_machine", data: { history_seconds: 10 } });

const GRAPH: PatchGraph = {
  nodes: [placed("radio", { kind: "device", data: { device: { backend: "virtual" } } }), NODE],
  edges: [{ from: { node: "radio", port: "iq" }, to: { node: "history", port: "iq" } }],
};

const RESOLVED = resolvedPatch([
  resolvedNodeOf("history", "time_machine", [
    resolvedPort(
      { name: "iq", port_type: "iq", direction: "in" },
      { feeds: [{ source: "radio", port: "iq", tap: { tap: "lane", device_set: 1, stream: 0 } }] },
    ),
  ]),
]);

function held(node: string): TimeMachineStatus {
  return {
    node,
    stream: 0,
    history_seconds: 10,
    sample_rate: 2_048_000,
    center_hz: 100_000_000,
    held_samples: 10_240_000,
    capacity_samples: 20_480_000,
    overruns: 0,
  };
}

const capture = {
  file: "tm_1_20261023T000000Z",
  stream: 0,
  started_at: "2026-10-23T00:00:00Z",
  samples: 10_240_000,
  bytes: 81_920_000,
  overruns: 0,
};

function face(set: DeviceSet): string {
  return renderFace(TimeMachineFace, NODE, {
    graph: GRAPH,
    devices: new Map([["radio", set]]),
    resolved: RESOLVED,
  });
}

describe("TimeMachineFace", () => {
  it("offers arm and the held window to the node that owns the buffer", () => {
    expect(face(deviceSet({ time_machine: null }))).toContain(">Arm<");
    const armed = face(deviceSet({ time_machine: held("history") }));
    expect(armed).toContain("Capture</button>");
    expect(armed).toContain(">Disarm<");
    const capturing = face(deviceSet({ time_machine: { ...held("history"), capture } }));
    expect(capturing).toContain(">Stop<");
  });

  it("offers no stop for a buffer another node holds", () => {
    const html = face(deviceSet({ time_machine: held("other") }));
    expect(html).not.toContain(">Stop<");
    expect(html).not.toContain(">Disarm<");
    expect(html).toContain("Another time machine already holds this history");
  });
});
