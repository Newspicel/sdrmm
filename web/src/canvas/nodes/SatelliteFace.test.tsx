import { afterEach, describe, expect, it } from "vitest";
import { useSatelliteStore } from "../../lib/satellite";
import type { PatchNode, ResolvedPatch } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { SatelliteFace } from "./SatelliteFace";

const SAT: PatchNode = placed("sat", { kind: "satellite", data: {} });

describe("SatelliteFace", () => {
  it("offers a search over a list that scrolls", () => {
    const html = renderFace(SatelliteFace, SAT, {
      graph: { nodes: [SAT], edges: [] },
      resolved: null,
      inspected: true,
    });
    expect(html).toContain('aria-label="Search satellites"');
    expect(html).toMatch(/aria-label="Satellites"[^>]*overflow-y-auto/);
  });

  it("changes the satellite from the footer", () => {
    const picked: PatchNode = placed("sat", {
      kind: "satellite",
      data: { tle: "ISS (ZARYA)\n1 25544U\n2 25544" },
    });
    const html = renderFace(SatelliteFace, picked, {
      graph: { nodes: [picked], edges: [] },
      resolved: null,
    });
    expect(html).toMatch(/title="Pick another satellite"[^>]*>Change</);
    expect(html).not.toContain("Wire a position");
  });

  it("tracks beside the dome once it has a position", () => {
    const picked: PatchNode = placed("sat", {
      kind: "satellite",
      data: { tle: "ISS (ZARYA)\n1 25544U\n2 25544" },
    });
    useSatelliteStore.getState().observe({
      type: "SatelliteUpdate",
      data: {
        status: {
          node: "sat",
          visible: true,
          look: { azimuth_deg: 123, elevation_deg: 21.5, range_km: 900, range_rate_km_s: -3 },
          doppler_hz: 4_200,
          pass_track: [],
        },
      },
    } as never);
    const resolved = {
      nodes: [
        {
          id: "sat",
          kind: "satellite",
          ports: [
            {
              name: "position",
              direction: "in",
              feeds: [{ source: "gps", port: "position", payload: { type: "position" } }],
            },
          ],
        },
      ],
    } as unknown as ResolvedPatch;
    const html = renderFace(SatelliteFace, picked, {
      graph: { nodes: [picked], edges: [] },
      resolved,
    });
    expect(html).not.toContain("Wire a position");
    expect(html).toContain("21.5°");
    expect(html).toContain("123° SE");
  });
});

afterEach(() => {
  useSatelliteStore.getState().reset();
});
