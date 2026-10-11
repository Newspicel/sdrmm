import type { Map as MapLibreMap } from "maplibre-gl";
import type { StationOf } from "../decoded";
import type { LatLon } from "../types";
import { unwrapTrail } from "./bounds";
import { isStale, KIND_STYLE, TARGET_MAX_AGE_MS } from "./layers";

export const FORECAST_SOURCE = "sonde-forecast";
export const FORECAST_LAYERS = [
  "sonde-forecast-path",
  "sonde-forecast-burst",
  "sonde-forecast-landing",
] as const;

type Mark = "path" | "burst" | "landing";
type Line = { type: "LineString"; coordinates: [number, number][] };
type Point = { type: "Point"; coordinates: [number, number] };

export interface ForecastCollection {
  type: "FeatureCollection";
  features: {
    type: "Feature";
    geometry: Line | Point;
    properties: { id: string; mark: Mark };
  }[];
}

const EMPTY: ForecastCollection = { type: "FeatureCollection", features: [] };

function lonLat(at: LatLon): [number, number] {
  return [at.lon, at.lat];
}

export function forecastCollection(
  sondes: readonly StationOf<"radiosonde">[],
  nowMs: number,
  maxAgeMs = TARGET_MAX_AGE_MS,
): ForecastCollection {
  const features: ForecastCollection["features"] = [];
  for (const sonde of sondes) {
    const forecast = sonde.event.data.forecast;
    if (forecast == null || isStale(sonde.lastSeen, nowMs, maxAgeMs)) {
      continue;
    }
    const id = sonde.id;
    const path = forecast.path ?? [];
    if (path.length >= 2) {
      features.push({
        type: "Feature",
        geometry: { type: "LineString", coordinates: unwrapTrail(path.map(lonLat)) },
        properties: { id, mark: "path" },
      });
    }
    features.push(
      {
        type: "Feature",
        geometry: { type: "Point", coordinates: lonLat(forecast.burst) },
        properties: { id, mark: "burst" },
      },
      {
        type: "Feature",
        geometry: { type: "Point", coordinates: lonLat(forecast.landing) },
        properties: { id, mark: "landing" },
      },
    );
  }
  return { type: "FeatureCollection", features };
}

function removeForecastLayers(map: MapLibreMap): void {
  for (const layer of FORECAST_LAYERS) {
    if (map.getLayer(layer) !== undefined) {
      map.removeLayer(layer);
    }
  }
  if (map.getSource(FORECAST_SOURCE) !== undefined) {
    map.removeSource(FORECAST_SOURCE);
  }
}

export function installForecastLayers(map: MapLibreMap, edge: string, enabled: boolean): void {
  removeForecastLayers(map);
  if (!enabled) {
    return;
  }
  const color = KIND_STYLE.radiosonde.color;
  map.addSource(FORECAST_SOURCE, { type: "geojson", data: EMPTY });
  map.addLayer({
    id: FORECAST_LAYERS[0],
    type: "line",
    source: FORECAST_SOURCE,
    filter: ["==", ["get", "mark"], "path"],
    layout: { "line-join": "round", "line-cap": "round" },
    paint: {
      "line-color": color,
      "line-width": 1.5,
      "line-opacity": 0.8,
      "line-dasharray": [2, 2],
    },
  });
  map.addLayer({
    id: FORECAST_LAYERS[1],
    type: "circle",
    source: FORECAST_SOURCE,
    filter: ["==", ["get", "mark"], "burst"],
    paint: {
      "circle-radius": 4,
      "circle-opacity": 0,
      "circle-stroke-color": color,
      "circle-stroke-width": 1.5,
    },
  });
  map.addLayer({
    id: FORECAST_LAYERS[2],
    type: "circle",
    source: FORECAST_SOURCE,
    filter: ["==", ["get", "mark"], "landing"],
    paint: {
      "circle-radius": 5,
      "circle-color": edge,
      "circle-stroke-color": color,
      "circle-stroke-width": 2.5,
    },
  });
}
