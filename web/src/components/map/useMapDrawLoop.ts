import type { GeoJSONSource } from "maplibre-gl";
import { type RefObject, useEffect } from "react";
import { paletteTable } from "../../gl/surface";
import type { FusionGridFrame } from "../../lib/frame";
import { drawHeat, type HeatScratch } from "../../lib/map/heat";
import { POSITION_ROUTE_SOURCE, POSITION_SOURCE } from "../../lib/map/position";
import { type PropagationOverlay, updatePropagationSources } from "../../lib/map/propagation";
import { SIGNAL_SOURCE } from "../../lib/map/signal";
import {
  framePositionOnce,
  frameSignalOnce,
  frameTargetsOnce,
  type TrackPoint,
  updatePositionSources,
  updateSignalSource,
} from "../../lib/map/sources";
import { usePositionStore } from "../../lib/position";
import type { Entity, PositionTrack, SurveyCell } from "../../lib/types";
import { drawSinkFeed } from "../../sinks/mapLayers";
import { type MapCore, type MapInputs, type MapSinks, readyCore } from "./mapState";

export const DRAW_TICK_MS = 500;
const FADE_REDRAW_MS = 5_000;
const HEAT_COLORMAP = "inferno";

interface Drawn {
  generation: number;
  entities: readonly Entity[] | null;
  tracksOn: boolean | null;
  entitiesAt: number;
  position: string;
  positions: readonly PositionTrack[] | null;
  signal: readonly SurveyCell[] | null;
  propagation: PropagationOverlay | null;
  heat: FusionGridFrame | null;
}

interface Painter {
  table: Uint8ClampedArray;
  scratch: HeatScratch;
}

function freshDrawn(generation: number): Drawn {
  return {
    generation,
    entities: null,
    tracksOn: null,
    entitiesAt: 0,
    position: "",
    positions: null,
    signal: null,
    propagation: null,
    heat: null,
  };
}

function trackPoints(track: PositionTrack): TrackPoint[] {
  return (track.trail ?? []).map((at, index) => ({
    latitude: at.lat,
    longitude: at.lon,
    receivedAt: index,
  }));
}

function storeTracks(nodes: readonly string[]) {
  const sources = usePositionStore.getState().sources;
  return nodes.map((node) => ({
    key: `${node}:${sources[node]?.history.length ?? 0}:${sources[node]?.history.at(-1)?.receivedAt ?? 0}:${sources[node]?.fix != null}`,
    samples: sources[node]?.history ?? [],
    active: sources[node]?.fix != null,
  }));
}

function drawPositions(core: MapCore, inputs: MapInputs, drawn: Drawn, sinks: MapSinks): void {
  let tracks: { samples: readonly TrackPoint[]; active: boolean }[];
  if (inputs.positions !== null) {
    if (inputs.positions === drawn.positions) {
      return;
    }
    drawn.positions = inputs.positions;
    tracks = inputs.positions.map((track) => ({
      samples: trackPoints(track),
      active: track.fix != null,
    }));
  } else {
    const fromStore = storeTracks(inputs.positionNodes);
    const key = fromStore.map((track) => track.key).join("|");
    if (key === drawn.position) {
      return;
    }
    drawn.position = key;
    tracks = fromStore;
  }
  const collection = updatePositionSources(
    core.map.getSource<GeoJSONSource>(POSITION_SOURCE),
    core.map.getSource<GeoJSONSource>(POSITION_ROUTE_SOURCE),
    tracks,
  );
  sinks.setPositionCount(collection.points.features.length);
  framePositionOnce(core.map, collection.points, core.framing.position);
}

function drawOverlays(core: MapCore, inputs: MapInputs, drawn: Drawn, painter: Painter): void {
  const propagation = inputs.propagation;
  if (propagation !== null && propagation !== drawn.propagation) {
    drawn.propagation = propagation;
    updatePropagationSources(core.map, propagation);
  }
  const heat = inputs.heat;
  if (heat !== null && heat !== drawn.heat) {
    drawn.heat = heat;
    drawHeat(core.map, heat, painter.table, painter.scratch);
  }
  const samples = inputs.signalSamples;
  if (samples !== null && samples !== drawn.signal) {
    drawn.signal = samples;
    const source = core.map.getSource<GeoJSONSource>(SIGNAL_SOURCE);
    frameSignalOnce(core.map, updateSignalSource(source, samples), core.framing.signal);
  }
}

function drawEntities(core: MapCore, inputs: MapInputs, drawn: Drawn, now: number): void {
  const entities = inputs.entities;
  if (entities === null) {
    return;
  }
  const stale = now - drawn.entitiesAt >= FADE_REDRAW_MS;
  if (entities === drawn.entities && inputs.tracks === drawn.tracksOn && !stale) {
    return;
  }
  drawn.entities = entities;
  drawn.tracksOn = inputs.tracks;
  drawn.entitiesAt = now;
  const points = drawSinkFeed(core.map, entities, core.hues, inputs.tracks, now);
  frameTargetsOnce(core.map, points, core.framing.targets);
}

export function useMapDrawLoop(
  coreRef: RefObject<MapCore | null>,
  inputsRef: RefObject<MapInputs>,
  sinks: MapSinks,
): void {
  useEffect(() => {
    const painter: Painter = { table: paletteTable(HEAT_COLORMAP), scratch: { pixels: null } };
    let drawn = freshDrawn(-1);
    const timer = setInterval(() => {
      const core = readyCore(coreRef.current);
      if (core === null) {
        return;
      }
      if (drawn.generation !== core.generation) {
        drawn = freshDrawn(core.generation);
      }
      const inputs = inputsRef.current;
      drawPositions(core, inputs, drawn, sinks);
      drawOverlays(core, inputs, drawn, painter);
      drawEntities(core, inputs, drawn, Date.now());
    }, DRAW_TICK_MS);
    return () => clearInterval(timer);
  }, [coreRef, inputsRef, sinks]);
}
