import "maplibre-gl/dist/maplibre-gl.css";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useEventKinds } from "../lib/eventKinds";
import type { FusionGridFrame } from "../lib/frame";
import { useMapView } from "../lib/map/mapView";
import type { PropagationOverlay } from "../lib/map/propagation";
import { entityKey } from "../lib/sinks";
import type { Entity, EventKindEntry, PositionTrack, SurveyCell } from "../lib/types";
import { HUE_COLOR } from "../sinks/hues";
import { highlightEntity } from "../sinks/mapLayers";
import { EntityCard } from "./map/EntityCard";
import { type LegendGroup, MapLegend } from "./map/MapLegend";
import { MapSettings } from "./map/MapSettings";
import type { MapInputs, MapSinks } from "./map/mapState";
import { useMapDrawLoop } from "./map/useMapDrawLoop";
import { useLayerSync, useMapActive, useMapInstance } from "./map/useMapInstance";

export interface MapFeed {
  entities: readonly Entity[];
  tracks: readonly PositionTrack[];
  sourceName: (source: string) => string;
}

function colorOf(entity: Entity): string {
  return HUE_COLOR[entity.hue];
}

function nounOf(kind: string, entries: readonly EventKindEntry[]): string {
  return entries.find((entry) => entry.kind === kind)?.noun ?? kind;
}

function nameOf(kind: string, entries: readonly EventKindEntry[]): string {
  return entries.find((entry) => entry.kind === kind)?.name ?? kind;
}

export function legendGroups(
  feed: MapFeed,
  entries: readonly EventKindEntry[],
  paint: (entity: Entity) => string,
): LegendGroup[] {
  const groups = new Map<string, LegendGroup>();
  for (const entity of feed.entities) {
    if (entity.view.place == null) {
      continue;
    }
    const key = `${entity.key.source}\u0000${entity.key.kind}`;
    const group = groups.get(key);
    if (group === undefined) {
      groups.set(key, {
        key,
        label: `${feed.sourceName(entity.key.source)} · ${nounOf(entity.key.kind, entries)}`,
        color: paint(entity),
        count: 1,
      });
    } else {
      group.count += 1;
    }
  }
  return [...groups.values()].toSorted((a, b) => a.label.localeCompare(b.label));
}

export function MapPanel({
  feed,
  positionNodes = [],
  signalSamples,
  propagation,
  heat,
  heatRefused = null,
  active = true,
  className,
}: {
  feed?: MapFeed;
  positionNodes?: readonly string[];
  signalSamples?: readonly SurveyCell[];
  propagation?: PropagationOverlay;
  heat?: FusionGridFrame | null;
  heatRefused?: string | null;
  active?: boolean;
  className?: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const view = useMapView();
  const entries = useEventKinds();
  const inputs = (): MapInputs => ({
    entities: feed?.entities ?? null,
    positionNodes,
    positions: feed === undefined ? null : feed.tracks,
    signalSamples: signalSamples ?? null,
    propagation: propagation ?? null,
    heat: heat ?? null,
    heatEnabled: heat !== undefined,
    tracks: view.tracks,
    globe: view.globe,
  });
  const inputsRef = useRef<MapInputs>(inputs());
  useLayoutEffect(() => {
    inputsRef.current = inputs();
  });
  const [positionCount, setPositionCount] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const [headings, setHeadings] = useState(true);
  const sinks = useMemo<MapSinks>(() => ({ setPositionCount, setHeadings }), []);
  const { coreRef, basemap } = useMapInstance(
    containerRef,
    inputsRef,
    sinks,
    view.basemap,
    setSelected,
  );
  const close = () => {
    const core = coreRef.current;
    if (core !== null) {
      core.selected = null;
      highlightEntity(core.map, null);
    }
    setSelected(null);
  };
  const positionsOn = positionNodes.length > 0 || (feed?.tracks.length ?? 0) > 0;
  useLayerSync(
    coreRef,
    {
      entitiesOn: feed !== undefined,
      positionsOn,
      globe: view.globe,
      signalEnabled: signalSamples !== undefined,
      propagationLayer: propagation?.layer ?? null,
      heatEnabled: heat !== undefined,
    },
    sinks,
  );
  useMapDrawLoop(coreRef, inputsRef, sinks);
  useMapActive(containerRef, coreRef, active, basemap);
  const picked =
    selected === null
      ? undefined
      : feed?.entities.find((entity) => entityKey(entity.key) === selected);

  return (
    <div className={`relative ${className ?? "h-[min(60dvh,28rem)] min-h-64 w-full"}`}>
      <div ref={containerRef} className="h-full w-full bg-bg" />
      <MapLegend
        groups={feed === undefined ? [] : legendGroups(feed, entries, colorOf)}
        positionCount={positionsOn ? positionCount : null}
        signalCells={signalSamples === undefined ? null : signalSamples.length}
        heat={heat != null}
        heatRefused={heatRefused}
        headings={headings}
        basemap={basemap}
      />
      <div className="absolute bottom-2 left-2 flex items-center rounded-control bg-plot-bg/85 p-0.5">
        <MapSettings />
      </div>
      {picked !== undefined && (
        <EntityCard
          entity={picked}
          kindName={nameOf(picked.key.kind, entries)}
          color={colorOf(picked)}
          onClose={close}
        />
      )}
    </div>
  );
}
