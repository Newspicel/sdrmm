import { describe, expect, it } from "vitest";
import type { ChannelInfo, DeviceSet, PatchGraph, PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { deviceSet, placed } from "../../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort, wiredPatch } from "../../test/resolved";
import { ChannelFace } from "./ChannelFace";
import { EventOutputFace } from "./EventOutputFace";
import { AudioRecorderFace } from "./RecorderFaces";

const OUTPUT: PatchNode = placed("out", {
  kind: "event_output",
  data: { target: { service: "desktop" } },
});
const DECODER = placed("nfm", { kind: "channel", data: { channel_type: "nfm" } });
const WIRE_IN = "Wire events in";

function call(id: number): ChannelInfo {
  return {
    id,
    node: "trunk",
    stream: 0,
    settings: { frequency_hz: 451_000_000, params: { type: "dmr", settings: {} } as never },
  };
}

function fedOutput(sources: string[]) {
  return resolvedPatch([
    resolvedNodeOf("out", "event_output", [
      resolvedPort(
        { name: "events", port_type: "events", direction: "in", multi: "many" },
        { feeds: sources.map((source) => ({ source, port: "events" })) },
      ),
    ]),
  ]);
}

describe("faces read the resolved patch", () => {
  it("counts what feeds an input, not the wires drawn locally", () => {
    const drawn: PatchGraph = {
      nodes: [OUTPUT, DECODER],
      edges: [{ from: { node: "nfm", port: "events" }, to: { node: "out", port: "events" } }],
    };
    expect(
      renderFace(EventOutputFace, OUTPUT, { graph: drawn, resolved: fedOutput([]) }),
    ).toContain(WIRE_IN);
    expect(
      renderFace(EventOutputFace, OUTPUT, {
        graph: { nodes: [OUTPUT], edges: [] },
        resolved: fedOutput(["nfm"]),
      }),
    ).not.toContain(WIRE_IN);
  });

  it("marks a node with the problems the server found", () => {
    const html = renderFace(EventOutputFace, OUTPUT, {
      graph: { nodes: [OUTPUT], edges: [] },
      problems: [
        { node: "out", source: "bind", text: "Radio not open" },
        { node: "other", source: "bind", text: "Not placed on its radio" },
      ],
    });
    expect(html).toContain('aria-label="Radio not open"');
    expect(html).not.toContain("Not placed on its radio");
  });

  it("records every channel a trunk feeds into an audio recorder", () => {
    const recorder = placed("rec", {
      kind: "audio_recorder",
      data: { recording: true, skip_silence: false },
    });
    const set: DeviceSet = deviceSet({ id: 2, channels: [call(5), call(6)] });
    const resolved = resolvedPatch([
      resolvedNodeOf("rec", "audio_recorder", [
        resolvedPort(
          { name: "audio", port_type: "audio", direction: "in", multi: "many" },
          {
            feeds: [
              {
                source: "trunk",
                port: "audio",
                tap: {
                  tap: "channels",
                  channels: [
                    { device_set: 2, channel: 5 },
                    { device_set: 2, channel: 6 },
                  ],
                },
              },
            ],
          },
        ),
      ]),
    ]);
    const html = renderFace(AudioRecorderFace, recorder, {
      graph: { nodes: [recorder], edges: [] },
      devices: new Map([["radio", set]]),
      resolved,
    });
    expect(html.match(/>Waiting</g)).toHaveLength(2);
  });

  it("names the controller wired into a decoder's control input", () => {
    const radio = placed("radio", { kind: "device", data: { device: { backend: "virtual" } } });
    const scan = placed("scan", { kind: "scanner", data: {}, label: "Airband" });
    const graph: PatchGraph = {
      nodes: [radio, DECODER, scan],
      edges: [
        { from: { node: "radio", port: "iq" }, to: { node: "nfm", port: "iq" } },
        { from: { node: "scan", port: "control" }, to: { node: "nfm", port: "control" } },
      ],
    };
    const live: ChannelInfo = {
      id: 3,
      node: "nfm",
      stream: 0,
      settings: { frequency_hz: 118_000_000, params: { type: "nfm", settings: {} } as never },
    };
    const set = deviceSet({ channels: [live] });
    const wiring = {
      devices: new Map([["radio", set]]),
      channels: new Map([["nfm", live]]),
      owners: new Map([["nfm", "radio"]]),
    };
    const held = renderFace(ChannelFace, DECODER, { graph, ...wiring });
    expect(held).toContain("Tuned by Airband");
    const free = renderFace(ChannelFace, DECODER, {
      graph,
      ...wiring,
      resolved: wiredPatch({ ...graph, edges: graph.edges?.slice(0, 1) }, wiring),
    });
    expect(free).not.toContain("Tuned by");
  });
});
