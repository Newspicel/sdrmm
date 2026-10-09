import { describe, expect, it } from "vitest";
import type { DeviceSet, PatchGraph, RecordingStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { deviceSet, placed } from "../../test/fixtures";
import { RecorderFace } from "./SinkFaces";

const RADIO = { driver: "ad936x", key: "192.168.1.10:30431", label: "AntSDR AD9361" };

const TAKE: RecordingStatus = {
  bytes: 167_317_504,
  file: "rec_0_20261009T114426Z",
  overruns: 0,
  samples: 20_914_688,
  started_at: "2026-10-09T11:44:26Z",
};

function recorder(recording: boolean) {
  return placed("rec", { kind: "recorder", data: { recording } });
}

function render(wired: boolean, recording: RecordingStatus | null): string {
  const node = recorder(recording !== null);
  const device = placed("dev", {
    kind: "device",
    data: { device: { backend: RADIO.driver, key: RADIO.key } },
  });
  const graph: PatchGraph = {
    nodes: [device, node],
    edges: wired ? [{ from: { node: "dev", port: "iq" }, to: { node: "rec", port: "iq" } }] : [],
  };
  const devices = new Map<string, DeviceSet>([
    ["dev", deviceSet({ device: RADIO, settings: { sample_rate: 8e6 }, recording })],
  ]);
  return renderFace(RecorderFace, node, { graph, devices });
}

function rows(html: string): string[] {
  return ["File", "Time", "Written"].filter((label) => html.includes(`>${label}<`));
}

describe("RecorderFace", () => {
  it("keeps the same rows whether unwired, idle or recording", () => {
    const states = [render(false, null), render(true, null), render(true, TAKE)];
    for (const html of states) {
      expect(rows(html)).toEqual(["File", "Time", "Written"]);
    }
  });

  it("offers Record only once a device's IQ is wired in", () => {
    expect(render(false, null)).toMatch(
      /<button[^>]*disabled=""[^>]*title="Wire a device&#x27;s IQ in"/,
    );
    expect(render(true, null)).not.toContain('disabled=""');
  });

  it("reads out the file and size while recording", () => {
    const html = render(true, TAKE);
    expect(html).toContain(">rec_0_20261009T114426Z<");
    expect(html).toContain(">167.3 MB<");
    expect(html).toContain(">Stop<");
  });
});
