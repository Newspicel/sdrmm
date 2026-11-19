import { afterEach, describe, expect, it } from "vitest";
import { useAimStore, useSkyScanStore } from "../../lib/pointing";
import type { PatchNode, SkyScanStatus } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import { AimFace } from "./AimFace";
import { SkyScanFace } from "./SkyScanFace";

const SKY: PatchNode = placed("sky", { kind: "sky_scan", data: {} });
const AIM: PatchNode = placed("aim", { kind: "aim", data: {} });
const AIM_WIRED = resolvedPatch([
  resolvedNodeOf("aim", "aim", [
    resolvedPort(
      { name: "events", port_type: "events", direction: "in" },
      { feeds: [{ source: "adsb", port: "events" }] },
    ),
    resolvedPort(
      { name: "position", port_type: "position", direction: "in" },
      { feeds: [{ source: "gps", port: "position" }] },
    ),
  ]),
]);

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
    inspected: true,
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
    expect(markup.match(/<rect[^>]*><title>/g)).toHaveLength(2);
    expect(markup).toContain("10° 0°: -40.0 dBFS");
    expect(markup).toContain("Start");
  });

  it("offers to stop while it runs", () => {
    useSkyScanStore.getState().put(scan(true));
    expect(render(SkyScanFace, SKY)).toContain("Stop");
  });

  it("draws the grid it will measure before the first point", () => {
    useSkyScanStore.getState().put({ ...scan(true), cells: [] });
    expect(render(SkyScanFace, SKY).match(/vector-effect="non-scaling-stroke"/g)).toHaveLength(36);
  });
});

describe("AimFace", () => {
  it("shows no fault while unwired", () => {
    useAimStore.getState().put({ node: "aim", error: "dish: offline" });
    const markup = render(AimFace, AIM);
    expect(markup).not.toContain('role="alert"');
  });

  it("shows a wait faint and a fault red once wired", () => {
    const wired = { graph: { nodes: [AIM], edges: [] }, resolved: AIM_WIRED, inspected: true };
    useAimStore.getState().put({ node: "aim", waiting: "Waiting for a fix" });
    const waiting = renderFace(AimFace, AIM, wired);
    expect(waiting).toContain("Waiting for a fix");
    expect(waiting).not.toContain('role="alert"');
    useAimStore.getState().put({ node: "aim", error: "dish: offline" });
    expect(renderFace(AimFace, AIM, wired)).toContain('role="alert"');
  });
});
