import type { MapOptions } from "maplibre-gl";
import { recordEvent } from "../diagnostics";

export type MapStyle = Exclude<NonNullable<MapOptions["style"]>, string>;

export const OPENFREEMAP = "https://tiles.openfreemap.org";
export const BASEMAP_GLYPHS = `${OPENFREEMAP}/fonts/{fontstack}/{range}.pbf`;
export const BASEMAP_TIMEOUT_MS = 4_000;
export const RASTER_TILE_PX = 256;

export const BASEMAP_PRESETS = ["liberty", "bright", "positron", "dark", "fiord"] as const;

export type BasemapPreset = (typeof BASEMAP_PRESETS)[number];

export const PRESET_COLORS: Record<BasemapPreset, { land: string; water: string }> = {
  liberty: { land: "#f8f4f0", water: "#9ebdff" },
  bright: { land: "#f8f4f0", water: "#aecfe2" },
  positron: { land: "#f2f3f0", water: "#c2c8ca" },
  dark: { land: "#0c0c0c", water: "#1b1b1d" },
  fiord: { land: "#45516e", water: "#38435c" },
};

export interface BasemapChoice {
  preset: BasemapPreset | "custom";
  custom: string;
}

export const DEFAULT_BASEMAP: BasemapChoice = { preset: "liberty", custom: "" };

export type BasemapKind = "pending" | "online" | "blank";

export interface Basemap {
  kind: BasemapKind;
  style: MapStyle;
}

export function groundOf(choice: BasemapChoice, kind: BasemapKind, background: string): string {
  if (kind !== "online") {
    return background;
  }
  return choice.preset === "custom"
    ? PRESET_COLORS.liberty.land
    : PRESET_COLORS[choice.preset].land;
}

export function presetUrl(preset: BasemapPreset): string {
  return `${OPENFREEMAP}/styles/${preset}`;
}

export function isTileTemplate(url: string): boolean {
  return url.includes("{z}") || url.includes("{quadkey}");
}

export function isWebUrl(url: string): boolean {
  if (!URL.canParse(url)) {
    return false;
  }
  const { protocol } = new URL(url);
  return protocol === "https:" || protocol === "http:";
}

export function sameBasemap(a: BasemapChoice, b: BasemapChoice): boolean {
  return a.preset === b.preset && (a.preset !== "custom" || a.custom === b.custom);
}

export function withCustom(basemap: BasemapChoice, custom: string): BasemapChoice {
  if (custom !== "") {
    return { preset: "custom", custom };
  }
  return { preset: basemap.preset === "custom" ? DEFAULT_BASEMAP.preset : basemap.preset, custom };
}

export function blankStyle(background: string): MapStyle {
  return {
    version: 8,
    sources: {},
    layers: [{ id: "backdrop", type: "background", paint: { "background-color": background } }],
  };
}

export function rasterStyle(template: string): MapStyle {
  return {
    version: 8,
    glyphs: BASEMAP_GLYPHS,
    sources: { custom: { type: "raster", tiles: [template], tileSize: RASTER_TILE_PX } },
    layers: [{ id: "custom", type: "raster", source: "custom" }],
  };
}

export async function fetchStyle(url: string): Promise<MapStyle | null> {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(BASEMAP_TIMEOUT_MS) });
    if (!response.ok) {
      recordEvent("warn", "map", `basemap style: HTTP ${response.status}`);
      return null;
    }
    return (await response.json()) as MapStyle;
  } catch (error) {
    recordEvent("warn", "map", `basemap style: ${String(error)}`);
    return null;
  }
}

export function chooseBasemap(online: MapStyle | null, background: string): Basemap {
  if (online !== null) {
    return { kind: "online", style: online };
  }
  return { kind: "blank", style: blankStyle(background) };
}

export async function loadBasemap(choice: BasemapChoice, background: string): Promise<Basemap> {
  if (choice.preset !== "custom") {
    return chooseBasemap(await fetchStyle(presetUrl(choice.preset)), background);
  }
  const url = choice.custom.trim();
  if (!isWebUrl(url)) {
    recordEvent("warn", "map", "basemap: custom URL is not http(s)");
    return chooseBasemap(null, background);
  }
  if (isTileTemplate(url)) {
    return { kind: "online", style: rasterStyle(url) };
  }
  return chooseBasemap(await fetchStyle(url), background);
}
