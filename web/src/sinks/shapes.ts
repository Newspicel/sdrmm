import { unwrapTrail } from "../lib/map/bounds";
import type { LineStyle, MapSymbol, Shape } from "../lib/types";
import { circleRing, ellipseRing, type Lnglat, lnglat, rayLine } from "./geometry";

export interface LineLook {
  width: number;
  opacity: number;
  dashed: boolean;
}

export const LINE_LOOK: Readonly<Record<LineStyle, LineLook>> = {
  track: { width: 1.5, opacity: 0.7, dashed: false },
  forecast: { width: 1.5, opacity: 0.8, dashed: true },
  route: { width: 2, opacity: 0.9, dashed: true },
  footprint: { width: 1, opacity: 0.6, dashed: false },
};

const RAY_LOOK: LineLook = { width: 1.5, opacity: 0.8, dashed: false };
const FAINT_RAY_LOOK: LineLook = { width: 1, opacity: 0.35, dashed: true };
const RING_LOOK: LineLook = { width: 1, opacity: 0.7, dashed: false };

export interface LineProps {
  key: string;
  color: string;
  width: number;
  opacity: number;
  dashed: boolean;
}

export interface FillProps {
  key: string;
  color: string;
}

export interface PinProps {
  key: string;
  color: string;
  symbol: MapSymbol;
  label: string;
}

export interface Feature<G, P> {
  type: "Feature";
  geometry: G;
  properties: P;
}

export type LineFeature = Feature<{ type: "LineString"; coordinates: Lnglat[] }, LineProps>;
export type FillFeature = Feature<{ type: "Polygon"; coordinates: Lnglat[][] }, FillProps>;
export type PinFeature = Feature<{ type: "Point"; coordinates: Lnglat }, PinProps>;

export interface ShapeFeatures {
  lines: LineFeature[];
  fills: FillFeature[];
  pins: PinFeature[];
}

function line(coordinates: Lnglat[], key: string, color: string, look: LineLook): LineFeature {
  return {
    type: "Feature",
    geometry: { type: "LineString", coordinates },
    properties: { key, color, ...look },
  };
}

function ring(coordinates: Lnglat[], key: string, color: string, into: ShapeFeatures): void {
  into.fills.push({
    type: "Feature",
    geometry: { type: "Polygon", coordinates: [coordinates] },
    properties: { key, color },
  });
  into.lines.push(line(coordinates, key, color, RING_LOOK));
}

export function addShape(shape: Shape, key: string, color: string, into: ShapeFeatures): void {
  switch (shape.shape) {
    case "ray":
      into.lines.push(
        line(
          rayLine(shape.from, shape.bearing_deg, shape.length_m),
          key,
          color,
          shape.faint ? FAINT_RAY_LOOK : RAY_LOOK,
        ),
      );
      return;
    case "ellipse":
      if (shape.major_m > 0 && shape.minor_m > 0) {
        ring(
          ellipseRing(shape.center, shape.major_m, shape.minor_m, shape.bearing_deg),
          key,
          color,
          into,
        );
      }
      return;
    case "circle":
      if (shape.radius_m > 0) {
        ring(circleRing(shape.center, shape.radius_m), key, color, into);
      }
      return;
    case "line":
      if (shape.points.length >= 2) {
        into.lines.push(
          line(unwrapTrail(shape.points.map(lnglat)), key, color, LINE_LOOK[shape.style]),
        );
      }
      return;
    case "pin":
      into.pins.push({
        type: "Feature",
        geometry: { type: "Point", coordinates: lnglat(shape.at) },
        properties: { key, color, symbol: shape.symbol, label: shape.label },
      });
      return;
  }
}

export function emptyShapes(): ShapeFeatures {
  return { lines: [], fills: [], pins: [] };
}
