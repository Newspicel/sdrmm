import type { GeoJSONSource, Map as MapLibreMap, MapMouseEvent } from "maplibre-gl";
import { recordEvent } from "../lib/diagnostics";
import { unwrapTrail } from "../lib/map/bounds";
import { setSourceData } from "../lib/map/sources";
import { entityKey, lastSeenMs } from "../lib/sinks";
import type { Entity, Hue, MapSymbol } from "../lib/types";
import { type Lnglat, lnglat } from "./geometry";
import { addShape, emptyShapes, type ShapeFeatures } from "./shapes";
import {
  DOT_RADIUS,
  ICON_SCALE,
  iconName,
  LABEL_OFFSET_EM,
  labelInk,
  SYMBOL_ICON,
} from "./symbols";

export type HueColors = Readonly<Record<Hue, string>>;

export const SINK_SOURCES = {
  entities: "sink-entities",
  trails: "sink-trails",
  lines: "sink-lines",
  fills: "sink-fills",
  pins: "sink-pins",
} as const;

export const SINK_LAYERS = [
  "sink-fills",
  "sink-lines",
  "sink-trails",
  "sink-pins",
  "sink-pin-labels",
  "sink-entity-dot",
  "sink-entity-heading",
  "sink-entity-label",
] as const;

export const HIT_LAYERS = ["sink-entity-dot", "sink-entity-heading", "sink-entity-label"] as const;
export const HIT_SLOP_PX = 9;
export const FADE_AFTER_MS = 300_000;
const MIN_FRESH = 0.25;

const SYMBOLS = Object.keys(SYMBOL_ICON) as MapSymbol[];

export interface EntityProps {
  key: string;
  color: string;
  label: string;
  symbol: MapSymbol;
  offset: number;
  radius: number;
  heading?: number;
}

export interface EntityFeature {
  type: "Feature";
  geometry: { type: "Point"; coordinates: Lnglat };
  properties: EntityProps;
}

export interface EntityCollection {
  type: "FeatureCollection";
  features: EntityFeature[];
}

function collection<F>(features: F[]): { type: "FeatureCollection"; features: F[] } {
  return { type: "FeatureCollection", features };
}

export function entityLabel(entity: Entity): string {
  return entity.view.label ?? entity.key.id;
}

export function entityCollection(entities: readonly Entity[], colors: HueColors): EntityCollection {
  const features: EntityFeature[] = [];
  for (const entity of entities) {
    const place = entity.view.place;
    if (place == null || entity.symbol == null) {
      continue;
    }
    const heading = place.heading_deg ?? undefined;
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: lnglat(place.at) },
      properties: {
        key: entityKey(entity.key),
        color: colors[entity.hue],
        label: entityLabel(entity),
        symbol: entity.symbol,
        offset: LABEL_OFFSET_EM[entity.symbol],
        radius: DOT_RADIUS[entity.symbol],
        ...(heading === undefined ? {} : { heading }),
      },
    });
  }
  return collection(features);
}

export function trailCollection(entities: readonly Entity[], colors: HueColors) {
  return collection(
    entities.flatMap((entity) => {
      const points = (entity.trail ?? []).map(lnglat);
      const place = entity.view.place;
      if (place != null) {
        points.push(lnglat(place.at));
      }
      return points.length < 2
        ? []
        : [
            {
              type: "Feature" as const,
              geometry: { type: "LineString" as const, coordinates: unwrapTrail(points) },
              properties: { key: entityKey(entity.key), color: colors[entity.hue] },
            },
          ];
    }),
  );
}

export function freshness(entity: Entity, now: number): number {
  const age = Math.max(0, now - lastSeenMs(entity));
  return Math.max(MIN_FRESH, 1 - age / FADE_AFTER_MS);
}

export function shapeCollections(
  entities: readonly Entity[],
  colors: HueColors,
  now: number,
): ShapeFeatures {
  const shapes = emptyShapes();
  for (const entity of entities) {
    const key = entityKey(entity.key);
    const fresh = freshness(entity, now);
    const start = shapes.lines.length;
    for (const shape of entity.view.shapes ?? []) {
      addShape(shape, key, colors[entity.hue], shapes);
    }
    for (const line of shapes.lines.slice(start)) {
      line.properties.opacity *= fresh;
    }
  }
  return shapes;
}

function removeSinkLayers(map: MapLibreMap): void {
  for (const layer of SINK_LAYERS) {
    if (map.getLayer(layer) !== undefined) {
      map.removeLayer(layer);
    }
  }
  for (const source of Object.values(SINK_SOURCES)) {
    if (map.getSource(source) !== undefined) {
      map.removeSource(source);
    }
  }
}

function addIcons(map: MapLibreMap): boolean {
  let ok = true;
  for (const symbol of SYMBOLS) {
    const name = iconName(symbol);
    if (map.hasImage(name)) {
      continue;
    }
    const image = SYMBOL_ICON[symbol]();
    if (image === null) {
      ok = false;
      continue;
    }
    map.addImage(name, image, { pixelRatio: ICON_SCALE, sdf: true });
  }
  return ok;
}

function addShapeLayers(map: MapLibreMap, ground: string): void {
  const ink = labelInk(ground);
  map.addLayer({
    id: "sink-fills",
    type: "fill",
    source: SINK_SOURCES.fills,
    paint: { "fill-color": ["get", "color"], "fill-opacity": 0.12 },
  });
  map.addLayer({
    id: "sink-lines",
    type: "line",
    source: SINK_SOURCES.lines,
    filter: ["!", ["get", "dashed"]],
    layout: { "line-join": "round", "line-cap": "round" },
    paint: {
      "line-color": ["get", "color"],
      "line-width": ["get", "width"],
      "line-opacity": ["get", "opacity"],
    },
  });
  map.addLayer({
    id: "sink-trails",
    type: "line",
    source: SINK_SOURCES.trails,
    layout: { "line-join": "round", "line-cap": "round" },
    paint: { "line-color": ["get", "color"], "line-width": 1.5, "line-opacity": 0.7 },
  });
  map.addLayer({
    id: "sink-pins",
    type: "circle",
    source: SINK_SOURCES.pins,
    paint: {
      "circle-radius": 5,
      "circle-color": ["get", "color"],
      "circle-stroke-color": ink.halo,
      "circle-stroke-width": 1,
    },
  });
  map.addLayer({
    id: "sink-pin-labels",
    type: "symbol",
    source: SINK_SOURCES.pins,
    layout: {
      "text-field": ["get", "label"],
      "text-font": ["Noto Sans Bold"],
      "text-size": 11,
      "text-anchor": "top",
      "text-offset": [0, 0.7],
      "text-optional": true,
    },
    paint: { "text-color": ink.text, "text-halo-color": ink.halo, "text-halo-width": 1.4 },
  });
}

function addDashedLines(map: MapLibreMap): void {
  map.addLayer(
    {
      id: "sink-dashed",
      type: "line",
      source: SINK_SOURCES.lines,
      filter: ["get", "dashed"],
      paint: {
        "line-color": ["get", "color"],
        "line-width": ["get", "width"],
        "line-opacity": ["get", "opacity"],
        "line-dasharray": [2, 2],
      },
    },
    "sink-trails",
  );
}

function addEntityLayers(map: MapLibreMap, edge: string, ground: string): void {
  const ink = labelInk(ground);
  map.addLayer({
    id: "sink-entity-dot",
    type: "circle",
    source: SINK_SOURCES.entities,
    paint: {
      "circle-radius": ["get", "radius"],
      "circle-color": ["get", "color"],
      "circle-stroke-color": edge,
      "circle-stroke-width": 1,
    },
  });
  map.addLayer({
    id: "sink-entity-heading",
    type: "symbol",
    source: SINK_SOURCES.entities,
    filter: ["has", "heading"],
    layout: {
      "icon-image": ["concat", "sink-symbol-", ["get", "symbol"]],
      "icon-rotate": ["get", "heading"],
      "icon-rotation-alignment": "map",
      "icon-allow-overlap": true,
      "icon-ignore-placement": true,
    },
    paint: { "icon-color": ["get", "color"], "icon-halo-color": edge, "icon-halo-width": 1 },
  });
  map.addLayer({
    id: "sink-entity-label",
    type: "symbol",
    source: SINK_SOURCES.entities,
    layout: {
      "text-field": ["get", "label"],
      "text-font": ["Noto Sans Bold"],
      "text-size": 12,
      "text-variable-anchor": ["top"],
      "text-radial-offset": ["get", "offset"],
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

export function installSinkLayers(
  map: MapLibreMap,
  edge: string,
  ground: string,
  enabled: boolean,
): boolean {
  removeSinkLayers(map);
  if (map.getLayer("sink-dashed") !== undefined) {
    map.removeLayer("sink-dashed");
  }
  if (!enabled) {
    return true;
  }
  const empty = collection([]);
  for (const source of Object.values(SINK_SOURCES)) {
    map.addSource(source, { type: "geojson", data: empty });
  }
  const icons = addIcons(map);
  if (!icons) {
    recordEvent("warn", "map", "heading icons failed");
  }
  addShapeLayers(map, ground);
  addDashedLines(map);
  addEntityLayers(map, edge, ground);
  return icons;
}

export function drawSinkFeed(
  map: MapLibreMap,
  entities: readonly Entity[],
  colors: HueColors,
  trails: boolean,
  now: number,
): EntityCollection {
  const points = entityCollection(entities, colors);
  const shapes = shapeCollections(entities, colors, now);
  const source = (id: string) => map.getSource<GeoJSONSource>(id);
  setSourceData(source(SINK_SOURCES.entities), points);
  setSourceData(source(SINK_SOURCES.lines), collection(shapes.lines));
  setSourceData(source(SINK_SOURCES.fills), collection(shapes.fills));
  setSourceData(source(SINK_SOURCES.pins), collection(shapes.pins));
  setSourceData(
    source(SINK_SOURCES.trails),
    trails ? trailCollection(entities, colors) : collection([]),
  );
  return points;
}

export function hitEntity(map: MapLibreMap, event: MapMouseEvent): string | null {
  const layers = HIT_LAYERS.filter((id) => map.getLayer(id) !== undefined);
  if (layers.length === 0) {
    return null;
  }
  const { x, y } = event.point;
  const hit = map.queryRenderedFeatures(
    [
      [x - HIT_SLOP_PX, y - HIT_SLOP_PX],
      [x + HIT_SLOP_PX, y + HIT_SLOP_PX],
    ],
    { layers: [...layers] },
  )[0];
  const key: unknown = hit?.properties.key;
  return typeof key === "string" ? key : null;
}

export function highlightEntity(map: MapLibreMap | null, key: string | null): void {
  if (map?.getLayer("sink-entity-dot") === undefined) {
    return;
  }
  map.setPaintProperty(
    "sink-entity-dot",
    "circle-stroke-width",
    key === null ? 1 : ["case", ["==", ["get", "key"], key], 3, 1],
  );
}
