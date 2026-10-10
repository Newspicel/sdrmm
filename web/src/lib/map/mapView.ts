import { create } from "zustand";
import { BASEMAP_PRESETS, type BasemapChoice, DEFAULT_BASEMAP } from "./basemap";

export const MAP_VIEW_KEY = "sdrmm.mapView";

export interface MapView {
  basemap: BasemapChoice;
  tracks: boolean;
}

export const DEFAULT_MAP_VIEW: MapView = { basemap: DEFAULT_BASEMAP, tracks: true };

export const useMapView = create<MapView>(() => readMapView());

export function setMapView(change: Partial<MapView>): void {
  const next = { ...useMapView.getState(), ...change };
  try {
    localStorage.setItem(MAP_VIEW_KEY, JSON.stringify(next));
  } catch {}
  useMapView.setState(next, true);
}

export function parseMapView(stored: string | null): MapView {
  if (stored === null) {
    return DEFAULT_MAP_VIEW;
  }
  try {
    const value: unknown = JSON.parse(stored);
    if (typeof value !== "object" || value === null) {
      return DEFAULT_MAP_VIEW;
    }
    const { basemap, tracks } = value as Record<string, unknown>;
    return {
      basemap: parseBasemap(basemap),
      tracks: typeof tracks === "boolean" ? tracks : DEFAULT_MAP_VIEW.tracks,
    };
  } catch {
    return DEFAULT_MAP_VIEW;
  }
}

function parseBasemap(value: unknown): BasemapChoice {
  if (typeof value !== "object" || value === null) {
    return DEFAULT_BASEMAP;
  }
  const { preset, custom } = value as Record<string, unknown>;
  const known = preset === "custom" || BASEMAP_PRESETS.some((name) => name === preset);
  return {
    preset: known ? (preset as BasemapChoice["preset"]) : DEFAULT_BASEMAP.preset,
    custom: typeof custom === "string" ? custom : "",
  };
}

function readMapView(): MapView {
  try {
    return parseMapView(localStorage.getItem(MAP_VIEW_KEY));
  } catch {
    return DEFAULT_MAP_VIEW;
  }
}
