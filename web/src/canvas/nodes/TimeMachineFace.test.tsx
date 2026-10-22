import { describe, expect, it } from "vitest";
import type { DeviceSet, PatchGraph, PatchNode, TimeMachineStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { deviceSet, placed } from "../../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import { TimeMachineFace } from "./TimeMachineFace";

function node(armed: boolean): PatchNode {
  return placed("history", { kind: "time_machine", data: { history_seconds: 10, armed } });
}

function graph(armed: boolean): PatchGraph {
  return {
    nodes: [
      placed("radio", { kind: "device", data: { device: { backend: "virtual" } } }),
      node(armed),
    ],
    edges: [{ from: { node: "radio", port: "iq" }, to: { node: "history", port: "iq" } }],
  };
}

const RESOLVED = resolvedPatch([
  resolvedNodeOf("history", "time_machine", [
    resolvedPort(
      { name: "iq", port_type: "iq", direction: "in" },
      { feeds: [{ source: "radio", port: "iq", tap: { tap: "lane", device_set: 1, stream: 0 } }] },
    ),
  ]),
]);

function held(owner: string): TimeMachineStatus {
  return {
    node: owner,
    stream: 0,
    history_seconds: 10,
    sample_rate: 2_048_000,
    center_hz: 100_000_000,
    held_samples: 10_240_000,
    capacity_samples: 20_480_000,
    overruns: 0,
  };
}

const CAPTURE = {
  file: "tm_1_20261023T000000Z",
  stream: 0,
  started_at: "2026-10-23T00:00:00Z",
  samples: 10_240_000,
  bytes: 81_920_000,
  overruns: 0,
};

function face(armed: boolean, set: DeviceSet): string {
  return renderFace(TimeMachineFace, node(armed), {
    graph: graph(armed),
    devices: new Map([["radio", set]]),
    resolved: RESOLVED,
  });
}

describe("TimeMachineFace", () => {
  it("arms by the patch and reads the window the radio holds", () => {
    expect(face(false, deviceSet())).toContain(">Arm<");
    const waiting = face(true, deviceSet());
    expect(waiting).toContain("Waiting");
    expect(waiting).toContain(">Disarm<");
    const armed = face(true, deviceSet({ time_machine: held("history") }));
    expect(armed).toContain("Capture</button>");
    expect(armed).toContain("5.0 s");
    expect(
      face(true, deviceSet({ time_machine: { ...held("history"), capture: CAPTURE } })),
    ).toContain(">Stop<");
  });

  it("offers no controls for a window another node holds", () => {
    const html = face(false, deviceSet({ time_machine: held("other") }));
    expect(html).not.toContain(">Stop<");
    expect(html).not.toContain(">Disarm<");
    expect(html).toContain("Another time machine already holds this history");
    expect(html).toMatch(/disabled=""[^>]*>Arm</);
  });
});
