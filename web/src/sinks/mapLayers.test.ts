import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DecoderEvent, Entity, Hue, Presentation } from "../lib/types";
import { sinkEntity } from "../test/sinks";
import { entityCollection, type HueColors, installSinkLayers, shapeCollections } from "./mapLayers";

const COLORS = {
  accent: "#0000ff",
  muted: "#888888",
  blue: "#1111ff",
  cyan: "#00ffff",
  teal: "#21b0b0",
  green: "#00ff00",
  amber: "#ffbf00",
  orange: "#ff8800",
  violet: "#8800ff",
  indigo: "#4400ff",
  pink: "#ff00aa",
} satisfies HueColors;

const NOW = Date.parse("2026-09-21T10:00:00Z");
const ANY: DecoderEvent = { kind: "rtty", data: { text: "" } };

function placed(id: string, view: Partial<Presentation>, hue: Hue, symbol: Entity["symbol"]) {
  return {
    ...sinkEntity("source", "adsb", id, ANY, { summary: id, ...view }, hue),
    symbol,
  };
}

describe("map layers from a sink feed", () => {
  it("draws each entity by its symbol and hue", () => {
    const plane = placed(
      "A",
      { label: "DLH123", place: { at: { lat: 52.5, lon: 13.4 }, heading_deg: 90 } },
      "teal",
      "plane",
    );
    const ship = placed("B", { place: { at: { lat: 54, lon: 10 } } }, "amber", "ship");
    const unplaced = placed("C", {}, "teal", "plane");

    const points = entityCollection([plane, ship, unplaced], COLORS);

    expect(points.features.map((feature) => feature.properties.symbol)).toEqual(["plane", "ship"]);
    expect(points.features[0]?.properties).toMatchObject({
      color: "#21b0b0",
      label: "DLH123",
      heading: 90,
    });
    expect(points.features[1]?.properties.color).toBe("#ffbf00");
    expect(points.features[1]?.properties).not.toHaveProperty("heading");
  });

  it("turns rays, ellipses, circles and forecast lines into map features", () => {
    const at = { lat: 52.5, lon: 13.4 };
    const entity = placed(
      "S",
      {
        shapes: [
          { shape: "ray", from: at, bearing_deg: 45, length_m: 25_000, faint: false },
          { shape: "ellipse", center: at, major_m: 500, minor_m: 200, bearing_deg: 30 },
          { shape: "circle", center: at, radius_m: 2_000_000 },
          {
            shape: "line",
            points: [at, { lat: 52.6, lon: 13.5 }],
            style: "forecast",
          },
          { shape: "pin", at, symbol: "target", label: "Landing" },
        ],
      },
      "orange",
      "balloon",
    );
    entity.last_seen = new Date(NOW).toISOString();

    const shapes = shapeCollections([entity], COLORS, NOW);

    expect(shapes.lines).toHaveLength(4);
    expect(shapes.fills).toHaveLength(2);
    expect(shapes.pins.map((pin) => pin.properties.label)).toEqual(["Landing"]);
    const forecast = shapes.lines.find((line) => line.properties.dashed);
    expect(forecast?.geometry.coordinates).toEqual([
      [13.4, 52.5],
      [13.5, 52.6],
    ]);
  });

  it("fades the shapes of an entity that went quiet", () => {
    const entity = placed(
      "R",
      {
        shapes: [
          {
            shape: "ray",
            from: { lat: 0, lon: 0 },
            bearing_deg: 0,
            length_m: 1_000,
            faint: false,
          },
        ],
      },
      "indigo",
      "antenna",
    );
    entity.last_seen = new Date(NOW - 150_000).toISOString();

    const [ray] = shapeCollections([entity], COLORS, NOW).lines;

    expect(ray?.properties.opacity).toBeCloseTo(0.4, 5);
  });
});

interface FakeLayer {
  id: string;
  source: string;
}

function fakeMap() {
  const layers = new Map<string, FakeLayer>();
  const sources = new Set<string>();
  const images = new Set<string>();
  return {
    layers,
    sources,
    getLayer: (id: string) => layers.get(id),
    addLayer: (layer: FakeLayer) => {
      if (!sources.has(layer.source)) {
        throw new Error(`no source ${layer.source}`);
      }
      layers.set(layer.id, layer);
    },
    removeLayer: (id: string) => {
      if (!layers.delete(id)) {
        throw new Error(`no layer ${id}`);
      }
    },
    getSource: (id: string) => (sources.has(id) ? {} : undefined),
    addSource: (id: string) => {
      if (sources.has(id)) {
        throw new Error(`source ${id} is already added`);
      }
      sources.add(id);
    },
    removeSource: (id: string) => {
      for (const layer of layers.values()) {
        if (layer.source === id) {
          throw new Error(`source ${id} is still used by ${layer.id}`);
        }
      }
      sources.delete(id);
    },
    hasImage: (name: string) => images.has(name),
    addImage: (name: string) => images.add(name),
  };
}

describe("installSinkLayers", () => {
  beforeEach(() => {
    vi.stubGlobal("document", { createElement: () => ({ getContext: () => null }) });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("takes the dashed lines down with their source, so a re-install stays up", () => {
    const map = fakeMap();
    installSinkLayers(map as never, "#111111", "#222222", true);
    expect(map.layers.has("sink-dashed")).toBe(true);

    expect(() => installSinkLayers(map as never, "#111111", "#222222", true)).not.toThrow();
    expect(map.layers.has("sink-dashed")).toBe(true);
    expect(map.sources.size).toBe(5);
  });

  it("leaves no source behind when the feed is switched off", () => {
    const map = fakeMap();
    installSinkLayers(map as never, "#111111", "#222222", true);
    installSinkLayers(map as never, "#111111", "#222222", false);
    expect(map.layers.size).toBe(0);
    expect(map.sources.size).toBe(0);
  });
});
