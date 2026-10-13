import { afterEach, describe, expect, it } from "vitest";
import { useSinkStore } from "../../lib/sinks";
import type { CapturedImage, PatchNode } from "../../lib/types";
import { catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { VideoFace } from "./VideoFace";

const NODE: PatchNode = placed("screen", catalogBody("video"));

const PICTURE: CapturedImage = {
  id: 4,
  node: "sstv",
  sinks: ["screen"],
  device_set: 1,
  channel: 2,
  at: "2026-08-16T10:00:00Z",
  freq_hz: 14_230_000,
  source: "sstv",
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
});
