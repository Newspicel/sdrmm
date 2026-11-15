import { sampleColormap } from "../../gl/colormap";
import type { SkyCell, SkyScanParams } from "../../lib/types";

const TURN = 360;
const HEAT_MAP = "viridis";

export interface SkySlot {
  column: number;
  row: number;
}

/** Where a measured cell sits in the scan's grid, west to east and bottom to top. */
export function skySlot(settings: SkyScanParams, cell: SkyCell): SkySlot {
  const turned = (((cell.azimuth_deg - settings.azimuth_from_deg) % TURN) + TURN) % TURN;
  return {
    column: Math.round(turned / settings.step_deg),
    row: Math.round((cell.elevation_deg - settings.elevation_from_deg) / settings.step_deg),
  };
}

export function levelSpan(cells: readonly SkyCell[]): { low: number; high: number } | null {
  if (cells.length === 0) {
    return null;
  }
  const levels = cells.map((cell) => cell.level_dbfs);
  return { low: Math.min(...levels), high: Math.max(...levels) };
}

export function heatColor(level: number, span: { low: number; high: number }): string {
  const range = span.high - span.low;
  const [r, g, b] = sampleColormap(HEAT_MAP, range > 0 ? (level - span.low) / range : 1);
  return `rgb(${Math.round(r * 255)} ${Math.round(g * 255)} ${Math.round(b * 255)})`;
}
