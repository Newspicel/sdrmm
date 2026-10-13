import type { MapSymbol } from "../lib/types";

export const ICON_SCALE = 2;

const ARROW_PX = 18;
const PLANE_PX = 26;
const SHIP_PX = 22;

type Painter = () => ImageData | null;

export function rasterize(
  px: number,
  draw: (ctx: CanvasRenderingContext2D) => void,
): ImageData | null {
  const canvas = document.createElement("canvas");
  canvas.width = px * ICON_SCALE;
  canvas.height = px * ICON_SCALE;
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    return null;
  }
  ctx.scale(ICON_SCALE, ICON_SCALE);
  draw(ctx);
  return ctx.getImageData(0, 0, canvas.width, canvas.height);
}

function silhouette(
  ctx: CanvasRenderingContext2D,
  px: number,
  starboard: readonly (readonly [number, number])[],
): void {
  const outline = [...starboard, ...starboard.toReversed().map(([x, y]) => [px - x, y] as const)];
  ctx.beginPath();
  outline.forEach(([x, y], index) => (index === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
  ctx.fillStyle = "#ffffff";
  ctx.fill();
}

const arrow: Painter = () =>
  rasterize(ARROW_PX, (ctx) => {
    const mid = ARROW_PX / 2;
    silhouette(ctx, ARROW_PX, [
      [mid, 1.5],
      [mid + 3.5, 8],
      [mid, 6.5],
    ]);
  });

const plane: Painter = () =>
  rasterize(PLANE_PX, (ctx) =>
    silhouette(ctx, PLANE_PX, [
      [13, 1.6],
      [14.4, 4.2],
      [14.4, 9.4],
      [24.4, 14.2],
      [24.4, 16.2],
      [14.4, 13.6],
      [14.4, 19.2],
      [18.6, 21.8],
      [18.6, 23.4],
      [13.6, 22.4],
      [13, 23.6],
    ]),
  );

const ship: Painter = () =>
  rasterize(SHIP_PX, (ctx) =>
    silhouette(ctx, SHIP_PX, [
      [11, 1.8],
      [15.6, 7.4],
      [15.6, 17.2],
      [14.2, 19.8],
      [11, 19.8],
    ]),
  );

export const SYMBOL_ICON: Readonly<Record<MapSymbol, Painter>> = {
  plane,
  ship,
  station: arrow,
  balloon: arrow,
  drone: arrow,
  beacon: arrow,
  node: arrow,
  person: arrow,
  vehicle: arrow,
  satellite: arrow,
  antenna: arrow,
  fix: arrow,
  target: arrow,
  echo: arrow,
  dot: arrow,
};

export const LABEL_OFFSET_EM: Readonly<Record<MapSymbol, number>> = {
  plane: 1.3,
  ship: 1.1,
  station: 0.7,
  balloon: 0.7,
  drone: 0.7,
  beacon: 0.7,
  node: 0.7,
  person: 0.7,
  vehicle: 0.7,
  satellite: 0.9,
  antenna: 0.7,
  fix: 0.7,
  target: 0.7,
  echo: 0.7,
  dot: 0.5,
};

export const DOT_RADIUS: Readonly<Record<MapSymbol, number>> = {
  plane: 4,
  ship: 4,
  station: 4,
  balloon: 4,
  drone: 4,
  beacon: 5,
  node: 4,
  person: 4,
  vehicle: 4,
  satellite: 5,
  antenna: 4,
  fix: 5,
  target: 6,
  echo: 4,
  dot: 3,
};

export function iconName(symbol: MapSymbol): string {
  return `sink-symbol-${symbol}`;
}

const DARK_INK = { text: "#15171a", halo: "#ffffff" } as const;
const LIGHT_INK = { text: "#f4f6f8", halo: "#0c0d0f" } as const;
const LIGHT_GROUND = 0.4;

export function labelInk(ground: string): { text: string; halo: string } {
  const hex = /^#?([0-9a-f]{6})$/i.exec(ground.trim())?.[1];
  if (hex === undefined) {
    return DARK_INK;
  }
  const [r, g, b] = [0, 2, 4].map((at) => {
    const channel = Number.parseInt(hex.slice(at, at + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  const luminance = 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
  return luminance > LIGHT_GROUND ? DARK_INK : LIGHT_INK;
}
