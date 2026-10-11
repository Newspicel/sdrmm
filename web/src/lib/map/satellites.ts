import type { ExpressionSpecification, GeoJSONSource, Map as MapLibreMap } from "maplibre-gl";
import type { LatLon, SatelliteStatus } from "../types";
import { unwrapTrail } from "./bounds";
import { destination } from "./df";
import { setSourceData } from "./sources";
import { labelInk } from "./targets";

export const SATELLITE_COLOR = "#e0409a";
export const FOOTPRINT_POINTS = 96;

export const SATELLITE_SOURCES = {
  point: "satellite-point",
  footprint: "satellite-footprint",
  track: "satellite-track",
} as const;

export const SATELLITE_LAYERS = [
  "satellite-footprint-fill",
  "satellite-footprint-edge",
  "satellite-track",
  "satellite-point",
  "satellite-label",
] as const;

type Line = { type: "LineString"; coordinates: [number, number][] };
type Point = { type: "Point"; coordinates: [number, number] };
type Polygon = { type: "Polygon"; coordinates: [number, number][][] };

interface Collection<G, P> {
  type: "FeatureCollection";
  features: { type: "Feature"; geometry: G; properties: P }[];
}

export interface SatelliteCollections {
  point: Collection<Point, { id: string; label: string; visible: boolean }>;
  footprint: Collection<Polygon, { id: string; visible: boolean }>;
  track: Collection<Line, { id: string }>;
}

const EMPTY = { type: "FeatureCollection", features: [] } as const;

export function satelliteLabel(status: SatelliteStatus): string {
  return status.name?.trim() || status.catalog || status.node;
}

export function footprintRing(at: LatLon, radiusKm: number): [number, number][] {
  const ring: [number, number][] = [];
  for (let step = 0; step <= FOOTPRINT_POINTS; step += 1) {
    ring.push(destination(at.lat, at.lon, (360 * step) / FOOTPRINT_POINTS, radiusKm * 1_000));
  }
  const unwrapped = unwrapTrail(ring);
  const first = unwrapped[0];
  const last = unwrapped.at(-1);
  if (first === undefined || last === undefined || Math.abs(last[0] - first[0]) < 180) {
    return unwrapped;
  }
  const pole = at.lat >= 0 ? 90 : -90;
  return [...unwrapped, [last[0], pole], [first[0], pole], first];
}

export function satelliteCollections(statuses: readonly SatelliteStatus[]): SatelliteCollections {
  const placed = statuses.filter(
    (status): status is SatelliteStatus & { ground: NonNullable<SatelliteStatus["ground"]> } =>
      status.ground != null,
  );
  return {
    point: {
      type: "FeatureCollection",
      features: placed.map((status) => ({
        type: "Feature",
        geometry: { type: "Point", coordinates: [status.ground.at.lon, status.ground.at.lat] },
        properties: {
          id: status.node,
          label: satelliteLabel(status),
          visible: status.visible === true,
        },
      })),
    },
    footprint: {
      type: "FeatureCollection",
      features: placed.map((status) => ({
        type: "Feature",
        geometry: {
          type: "Polygon",
          coordinates: [footprintRing(status.ground.at, status.ground.footprint_km)],
        },
        properties: { id: status.node, visible: status.visible === true },
      })),
    },
    track: {
      type: "FeatureCollection",
      features: statuses
        .filter((status) => (status.ground_track?.length ?? 0) >= 2)
        .map((status) => ({
          type: "Feature",
          geometry: {
            type: "LineString",
            coordinates: unwrapTrail(
              (status.ground_track ?? []).map((at) => [at.lon, at.lat] as [number, number]),
            ),
          },
          properties: { id: status.node },
        })),
    },
  };
}

export function drawSatellites(map: MapLibreMap, statuses: readonly SatelliteStatus[]): number {
  const collections = satelliteCollections(statuses);
  setSourceData(map.getSource<GeoJSONSource>(SATELLITE_SOURCES.point), collections.point);
  setSourceData(map.getSource<GeoJSONSource>(SATELLITE_SOURCES.footprint), collections.footprint);
  setSourceData(map.getSource<GeoJSONSource>(SATELLITE_SOURCES.track), collections.track);
  return collections.point.features.length;
}

function removeSatelliteLayers(map: MapLibreMap): void {
  for (const layer of SATELLITE_LAYERS) {
    if (map.getLayer(layer) !== undefined) {
      map.removeLayer(layer);
    }
  }
  for (const source of Object.values(SATELLITE_SOURCES)) {
    if (map.getSource(source) !== undefined) {
      map.removeSource(source);
    }
  }
}

export function installSatelliteLayers(
  map: MapLibreMap,
  edge: string,
  ground: string,
  enabled: boolean,
): void {
  removeSatelliteLayers(map);
  if (!enabled) {
    return;
  }
  for (const source of Object.values(SATELLITE_SOURCES)) {
    map.addSource(source, { type: "geojson", data: EMPTY });
  }
  const seen: ExpressionSpecification = ["get", "visible"];
  map.addLayer({
    id: SATELLITE_LAYERS[0],
    type: "fill",
    source: SATELLITE_SOURCES.footprint,
    paint: {
      "fill-color": SATELLITE_COLOR,
      "fill-opacity": ["case", seen, 0.22, 0.1],
    },
  });
  map.addLayer({
    id: SATELLITE_LAYERS[1],
    type: "line",
    source: SATELLITE_SOURCES.footprint,
    paint: {
      "line-color": SATELLITE_COLOR,
      "line-width": 1,
      "line-opacity": ["case", seen, 1, 0.75],
    },
  });
  map.addLayer({
    id: SATELLITE_LAYERS[2],
    type: "line",
    source: SATELLITE_SOURCES.track,
    layout: { "line-join": "round", "line-cap": "round" },
    paint: {
      "line-color": SATELLITE_COLOR,
      "line-width": 2,
      "line-opacity": 0.9,
      "line-dasharray": [3, 2],
    },
  });
  map.addLayer({
    id: SATELLITE_LAYERS[3],
    type: "circle",
    source: SATELLITE_SOURCES.point,
    paint: {
      "circle-radius": 5,
      "circle-color": SATELLITE_COLOR,
      "circle-stroke-color": edge,
      "circle-stroke-width": 1.5,
    },
  });
  const ink = labelInk(ground);
  map.addLayer({
    id: SATELLITE_LAYERS[4],
    type: "symbol",
    source: SATELLITE_SOURCES.point,
    layout: {
      "text-field": ["get", "label"],
      "text-font": ["Noto Sans Bold"],
      "text-size": 12,
      "text-anchor": "top",
      "text-offset": [0, 0.8],
      "text-optional": true,
    },
    paint: {
      "text-color": ink.text,
      "text-halo-color": ink.halo,
      "text-halo-width": 1.6,
      "text-halo-blur": 0.4,
    },
  });
}
