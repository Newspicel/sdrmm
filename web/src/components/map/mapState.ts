import type { Map as MapLibreMap } from "maplibre-gl";
import type { FusionGridFrame } from "../../lib/frame";
import type { PropagationOverlay } from "../../lib/map/propagation";
import type { Entity, PositionTrack, SurveyCell } from "../../lib/types";
import type { HueColors } from "../../sinks/mapLayers";

export interface MapInputs {
  entities: readonly Entity[] | null;
  positionNodes: readonly string[];
  positions: readonly PositionTrack[] | null;
  signalSamples: readonly SurveyCell[] | null;
  propagation: PropagationOverlay | null;
  heat: FusionGridFrame | null;
  heatEnabled: boolean;
  tracks: boolean;
  globe: boolean;
}

export interface FrameFlag {
  current: boolean;
}

export interface MapCore {
  map: MapLibreMap;
  ready: boolean;
  generation: number;
  edge: string;
  ground: string;
  accent: string;
  hues: HueColors;
  framing: { targets: FrameFlag; position: FrameFlag; signal: FrameFlag };
  selected: string | null;
}

export interface MapSinks {
  setPositionCount: (count: number) => void;
  setHeadings: (ok: boolean) => void;
}

export function readyCore(core: MapCore | null): MapCore | null {
  return core !== null && core.ready ? core : null;
}
