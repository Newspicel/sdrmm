import { afterEach, describe, expect, it } from "vitest";
import { useSinkStore } from "../../lib/sinks";
import type { PatchNode, PropagationView } from "../../lib/types";
import { catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { PropagationFace } from "./PropagationFace";

const NODE: PatchNode = placed("prop", catalogBody("propagation"));

const VIEW: PropagationView = {
  cells: [
    {
      key: "GO56",
      latitude: 46.5,
      longitude: -45,
      weight: 1.6,
      decodes: 3,
      callsigns: 2,
      best_freq_hz: 14_074_000,
      best_snr_db: -8,
      measured_muf3000_mhz: 18.4,
      median_distance_km: 6_000,
      last_seen: "2026-08-16T12:00:00Z",
    },
  ],
  summary: {
    decodes: 3,
    grids: 2,
    callsigns: 2,
    bands: 1,
    best_freq_hz: 14_074_000,
    best_muf3000_mhz: 18.4,
    farthest_km: 6_012,
  },
  receiver: { lat: 52.5, lon: 13 },
};

function render(view: PropagationView | null): string {
  useSinkStore.setState({
    sinks: {
      prop: {
        entities: [],
        records: [],
        tracks: [],
        surfaces: [],
        pictures: [],
        propagation: view,
        lost: 0,
      },
    },
  });
  return renderFace(PropagationFace, NODE, { graph: { nodes: [NODE], edges: [] } });
}

describe("propagation face", () => {
  afterEach(() => useSinkStore.getState().reset());

  it("shows the model the server sent", () => {
    const html = render(VIEW);
    expect(html).toContain("GO56");
    expect(html).toContain("6012 km");
    expect(html).toContain("≥ 18.4");
  });

  it("shows an empty model until spots arrive", () => {
    const html = render(null);
    expect(html).toContain("No reflection points yet.");
  });
});
