import { afterEach, describe, expect, it } from "vitest";
import { useAimStore, useSkyScanStore } from "../../lib/pointing";
import type { PatchNode, SkyScanStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { AimFace } from "./AimFace";
import { SkyScanFace } from "./SkyScanFace";

const SKY: PatchNode = placed("sky", { kind: "sky_scan", data: {} });
const AIM: PatchNode = placed("aim", { kind: "aim", data: {} });

function scan(running: boolean): SkyScanStatus {
  return {
    node: "sky",
    running,
    columns: 36,
    rows: 1,
    cells: [
      { azimuth_deg: 0, elevation_deg: 0, level_dbfs: -80 },
      { azimuth_deg: 10, elevation_deg: 0, level_dbfs: -40 },
    ],
  };
}

function render(face: typeof SkyScanFace, node: PatchNode): string {
  return renderFace(face, node, {
    graph: { nodes: [node], edges: [] },
    resolved: null,
  });
}

afterEach(() => {
  useSkyScanStore.getState().reset();
  useAimStore.getState().reset();
});

describe("SkyScanFace", () => {
  it("draws each measured point and offers to start", () => {
    useSkyScanStore.getState().put(scan(false));
    const markup = render(SkyScanFace, SKY);
    expect(markup.match(/<rect/g)).toHaveLength(2);
    expect(markup).toContain("10° 0°: -40.0 dBFS");
    expect(markup).toContain("Start");
  });

  it("offers to stop while it runs", () => {
    useSkyScanStore.getState().put(scan(true));
    expect(render(SkyScanFace, SKY)).toContain("Stop");
  });
});

describe("AimFace", () => {
  it("says what it waits for", () => {
    useAimStore.getState().put({ node: "aim", error: "wire a position" });
    expect(render(AimFace, AIM)).toContain("wire a position");
  });
});
