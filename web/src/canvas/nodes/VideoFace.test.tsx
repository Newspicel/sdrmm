import { afterEach, describe, expect, it } from "vitest";
import { useSinkStore } from "../../lib/sinks";
import type { CapturedImage, ChannelDescriptor, ChannelInfo, PatchNode } from "../../lib/types";
import { catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { deviceSet, placed } from "../../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import { VideoFace } from "./VideoFace";

const NODE: PatchNode = placed("screen", catalogBody("video"));

const PICTURE: CapturedImage = {
  id: 4,
  node: "picture",
  sinks: ["screen"],
  device_set: 1,
  channel: 2,
  at: "2026-08-16T10:00:00Z",
  freq_hz: 14_230_000,
  source: "picture",
  mode: "Martin M1",
  width: 320,
  height: 256,
  lines: 256,
  complete: true,
  image: { url: "/api/images/4/png", media_type: "image/png" },
};

function render(pictures: CapturedImage[]): string {
  useSinkStore.setState({
    sinks: {
      screen: {
        entities: [],
        records: [],
        tracks: [],
        surfaces: [],
        pictures,
        propagation: null,
        lost: 0,
      },
    },
  });
  return renderFace(VideoFace, NODE, { graph: { nodes: [NODE], edges: [] } });
}

describe("video face", () => {
  afterEach(() => useSinkStore.getState().reset());

  it("shows the pictures its wire kept", () => {
    const html = render([PICTURE]);
    expect(html).toContain("/api/images/4/png");
    expect(html).toContain("Martin M1 320 × 256");
  });

  it("shows no gallery before a picture arrives", () => {
    expect(render([])).not.toContain("<img");
  });

  it("keeps an empty screen and its picture stat while unwired", () => {
    const html = render([]);
    expect(html).toContain('aria-label="No picture"');
    expect(html).toContain(">Picture<");
  });
});

const TV: ChannelInfo = {
  id: 2,
  node: "tv",
  stream: 0,
  settings: { frequency_hz: 514_000_000, params: { type: "dvbt", settings: {} } },
};

function fedBy(hasAudio: boolean): string {
  const descriptor: ChannelDescriptor = {
    type_id: "dvbt",
    name: "DVB-T/T2",
    band: { low_hz: -4_000_000, high_hz: 4_000_000 },
    input_rate_hz: 9_142_857,
    has_audio: hasAudio,
    has_video: true,
    settings: [],
  };
  const resolved = resolvedPatch([
    resolvedNodeOf("screen", "video", [
      resolvedPort(
        { name: "video", port_type: "video", direction: "in", multi: "many" },
        {
          feeds: [
            {
              source: "tv",
              port: "video",
              tap: { tap: "channels", channels: [{ device_set: 1, channel: 2 }] },
            },
          ],
        },
      ),
    ]),
  ]);
  return renderFace(VideoFace, NODE, {
    graph: { nodes: [NODE], edges: [] },
    devices: new Map([["radio", deviceSet({ id: 1, channels: [TV] })]]),
    channelTypes: [descriptor],
    resolved,
  });
}

describe("video sound", () => {
  it("plays the sound a television wire carries", () => {
    const html = fedBy(true);
    expect(html).toContain(">Sound<");
    expect(html).toContain('aria-label="Play"');
  });

  it("offers no sound for a decoder that makes none", () => {
    expect(fedBy(false)).not.toContain(">Sound<");
  });
});
