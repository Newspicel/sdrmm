import { describe, expect, it } from "vitest";
import type { DecoderEvent, Entity, Hue, Presentation } from "../lib/types";
import { sinkEntity } from "../test/sinks";
import { entityCollection, type HueColors, shapeCollections } from "./mapLayers";

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
