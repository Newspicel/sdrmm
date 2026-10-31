import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { usePositionStore } from "../../lib/position";
import type { DeviceSet, PatchGraph, PatchNode, RecordingInfo } from "../../lib/types";
import { catalogBody } from "../../test/catalog";
import { renderFace, stubWorkspace } from "../../test/faceHarness";
import { resolvedBy } from "../../test/recorded";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import { nodePorts } from "../graph";
import { makeArray } from "../makeArray";
import { arrayLaneRows } from "./arrayNode";
import { RecordedPosition, RecordingFace, RecordingFacts } from "./RecordingFace";

const LANES = 5;

const take: PatchNode = {
  id: "rec",
  kind: "device",
  data: { device: { backend: "recording", key: "take" } },
  position: { x: 0, y: 0 },
};

function played(lanes: number): DeviceSet {
  return {
    id: 7,
    device: { driver: "recording", key: "take", label: "take" },
    capabilities: {
      freq_ranges: [],
      sample_rates: [2_400_000],
      gains: [],
      antennas: [],
      bandwidths: [],
      extra: [],
      duplex: "rx_only",
      rx_streams: lanes,
      coherence: "time_sync",
      noise_source: "replayed",
    },
    settings: {},
    status: "running",
    channels: [],
    overruns: 0,
  };
}

function collection(lanes: number): RecordingInfo {
  return {
    id: 1,
    file: "take",
    device_id: "recording:take",
    device_label: "virtual:kraken5",
    center_hz: 433_920_000,
    sample_rate: 2_400_000,
    samples: 2_400_000,
    bytes: 2_400_000 * 8 * lanes,
    duration_s: 1,
    created_at: "2026-09-29T12:00:00Z",
    lanes,
  };
}

const alone: PatchGraph = { nodes: [take], edges: [] };

describe("RecordingFace", () => {
  it("draws the iq outputs the server resolved for the recording", () => {
    const lanes = Array.from({ length: LANES }, (_, lane) =>
      resolvedPort({
        name: lane === 0 ? "iq" : `iq${lane + 1}`,
        port_type: "iq",
        direction: "out",
      }),
    );
    const resolved = resolvedPatch([resolvedNodeOf("rec", "device", lanes)]);
    const context = stubWorkspace({ graph: alone, resolved }).context;
    expect(nodePorts(context, take).map((port) => port.name)).toEqual([
      "iq",
      "iq2",
      "iq3",
      "iq4",
      "iq5",
    ]);
    const unresolved = stubWorkspace({ graph: alone }).context;
    expect(nodePorts(unresolved, take).map((port) => port.name)).toEqual([
      "iq",
      "control",
      "position",
    ]);
  });

  it("offers Make array for a playing collection and wires every lane", () => {
    const devices = new Map([["rec", played(LANES)]]);
    expect(renderFace(RecordingFace, take, { graph: alone, devices })).toContain("Make array");
    const single = renderFace(RecordingFace, take, {
      graph: alone,
      devices: new Map([["rec", played(1)]]),
    });
    expect(single).not.toContain("Make array");

    const body = catalogBody("array");
    if (body.kind !== "array") {
      throw new Error("an array body");
    }
    const offer = resolvedBy(alone, devices).nodes.find((node) => node.id === "rec")?.array_offer;
    if (offer?.offer !== "ready") {
      throw new Error("the server offers an array");
    }
    const made = makeArray(alone, "rec", offer.wires, body, "arr");
    const rows = arrayLaneRows(made, resolvedBy(made), devices, "arr", undefined);
    expect(rows.map((row) => row.sourceLabel)).toEqual([
      "take iq1",
      "take iq2",
      "take iq3",
      "take iq4",
      "take iq5",
    ]);
    expect(renderFace(RecordingFace, take, { graph: made, devices })).not.toContain("Make array");
  });

  it("reads out the recorded position once one is wired", () => {
    usePositionStore.getState().clear();
    expect(renderToStaticMarkup(<RecordedPosition node="rec" />)).toBe("");
    const at = (fix: { latitude: number; longitude: number } | null, error: string | null) =>
      usePositionStore.getState().observe({
        type: "PositionChanged",
        data: {
          node: "rec",
          fix: fix === null ? null : { ...fix, time: "2026-08-14T12:00:00Z" },
          error,
        },
      });
    at({ latitude: 52.5, longitude: 13.4 }, null);
    const shown = renderToStaticMarkup(<RecordedPosition node="rec" />);
    expect(shown).toContain("52.500000, 13.400000");
    expect(shown).toContain("JO62");
    at(null, "no position recorded here");
    expect(renderToStaticMarkup(<RecordedPosition node="rec" />)).toContain(
      "no position recorded here",
    );
    usePositionStore.getState().clear();
  });

  it("reads out the lane count of a collection only", () => {
    const five = renderToStaticMarkup(<RecordingFacts recording={collection(LANES)} />);
    expect(five).toContain("Lanes");
    expect(five).toContain(">5<");
    const one = renderToStaticMarkup(<RecordingFacts recording={collection(1)} />);
    expect(one).not.toContain("Lanes");
    expect(one).toContain("Centre");
  });
});
