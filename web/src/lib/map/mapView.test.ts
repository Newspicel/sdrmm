import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_BASEMAP } from "./basemap";
import { DEFAULT_MAP_VIEW, MAP_VIEW_KEY, parseMapView, setMapView, useMapView } from "./mapView";

describe("parseMapView", () => {
  it("falls back to defaults for missing or broken storage", () => {
    expect(parseMapView(null)).toEqual(DEFAULT_MAP_VIEW);
    expect(parseMapView("{nope")).toEqual(DEFAULT_MAP_VIEW);
    expect(parseMapView("42")).toEqual(DEFAULT_MAP_VIEW);
  });

  it("drops unknown presets but keeps the custom URL", () => {
    const stored = JSON.stringify({ basemap: { preset: "nope", custom: "https://x" }, tracks: 1 });
    expect(parseMapView(stored)).toEqual({
      basemap: { preset: DEFAULT_BASEMAP.preset, custom: "https://x" },
      tracks: DEFAULT_MAP_VIEW.tracks,
    });
  });
});

describe("setMapView", () => {
  afterEach(() => {
    localStorage.clear();
    useMapView.setState(DEFAULT_MAP_VIEW, true);
  });

  it("persists what it changes", () => {
    setMapView({ tracks: false });
    setMapView({ basemap: { preset: "fiord", custom: "" } });
    expect(useMapView.getState()).toEqual({
      basemap: { preset: "fiord", custom: "" },
      tracks: false,
    });
    expect(parseMapView(localStorage.getItem(MAP_VIEW_KEY))).toEqual(useMapView.getState());
  });
});
