import type { Rgb } from "../gl/colormap";

export interface Hsv {
  h: number;
  s: number;
  v: number;
}

export function rgbToHsv([r, g, b]: Rgb): Hsv {
  const max = Math.max(r, g, b);
  const delta = max - Math.min(r, g, b);
  return { h: hueOf(r, g, b, max, delta), s: max === 0 ? 0 : delta / max, v: max };
}

export function hsvToRgb({ h, s, v }: Hsv): Rgb {
  const channel = (n: number): number => {
    const k = (n + h / 60) % 6;
    return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
  };
  return [channel(5), channel(3), channel(1)];
}

function hueOf(r: number, g: number, b: number, max: number, delta: number): number {
  if (delta === 0) {
    return 0;
  }
  const sector =
    max === r ? (g - b) / delta : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4;
  return (sector * 60 + 360) % 360;
}
