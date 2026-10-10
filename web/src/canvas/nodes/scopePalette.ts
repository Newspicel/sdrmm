import {
  COLORMAPS,
  type Colormap,
  CUSTOM_STOPS,
  DEFAULT_COLORMAP,
  DEFAULT_CUSTOM,
  hexToRgb,
  type Palette,
  type Rgb,
  rgbToHex,
} from "../../gl/colormap";

export const CUSTOM = "custom";

export type PaletteChoice = Colormap | typeof CUSTOM;

const CHOICE_KEY = "sdrmm.colormap";
const CUSTOM_KEY = "sdrmm.colormapCustom";

export function paletteOf(choice: PaletteChoice, custom: readonly Rgb[]): Palette {
  return choice === CUSTOM ? custom : choice;
}

export function readPalette(): Palette {
  return paletteOf(readChoice(), readCustom());
}

export function readChoice(): PaletteChoice {
  const stored = readKey(CHOICE_KEY);
  if (stored === CUSTOM) {
    return CUSTOM;
  }
  return COLORMAPS.find((name) => name === stored) ?? DEFAULT_COLORMAP;
}

export function readCustom(): readonly Rgb[] {
  return parseCustom(readKey(CUSTOM_KEY)) ?? DEFAULT_CUSTOM;
}

export function storeChoice(choice: PaletteChoice): void {
  writeKey(CHOICE_KEY, choice);
}

export function storeCustom(stops: readonly Rgb[]): void {
  writeKey(CUSTOM_KEY, JSON.stringify(stops.map(rgbToHex)));
}

export function parseCustom(stored: string | null): readonly Rgb[] | null {
  if (stored === null) {
    return null;
  }
  let hexes: unknown;
  try {
    hexes = JSON.parse(stored);
  } catch {
    return null;
  }
  if (!Array.isArray(hexes) || hexes.length !== CUSTOM_STOPS) {
    return null;
  }
  const stops = hexes.map((hex) => (typeof hex === "string" ? hexToRgb(hex) : null));
  return stops.every((stop) => stop !== null) ? stops : null;
}

function readKey(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeKey(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {}
}
