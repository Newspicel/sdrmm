import type { RangeDopplerFrame } from "../../../lib/frame";
import { LIGHT_SPEED_M_S } from "../../../lib/limits";

export interface PlotAxes {
  rangeFirstM: number;
  rangeStepM: number;
  ranges: number;
  dopplerFirstHz: number;
  dopplerStepHz: number;
  dopplers: number;
  carrierHz: number;
  dbMin: number;
  dbMax: number;
}

export interface Box {
  w: number;
  h: number;
}

export interface SurfacePoint {
  rangeKm: number;
  dopplerHz: number;
}

export function frameAxes(frame: RangeDopplerFrame): PlotAxes {
  return {
    rangeFirstM: frame.rangeFirstM,
    rangeStepM: frame.rangeStepM,
    ranges: frame.ranges,
    dopplerFirstHz: frame.dopplerFirstHz,
    dopplerStepHz: frame.dopplerStepHz,
    dopplers: frame.dopplers,
    carrierHz: frame.carrierHz,
    dbMin: frame.dbMin,
    dbMax: frame.dbMax,
  };
}

export function rangeKmAt(col: number, axes: PlotAxes): number {
  return (axes.rangeFirstM + col * axes.rangeStepM) / 1_000;
}

export function dopplerHzAt(row: number, axes: PlotAxes): number {
  return axes.dopplerFirstHz + row * axes.dopplerStepHz;
}

export function velocityMps(dopplerHz: number, carrierHz: number): number | null {
  return carrierHz > 0 ? -(LIGHT_SPEED_M_S / carrierHz) * dopplerHz : null;
}

export function rangeSpanKm(axes: PlotAxes): { lo: number; hi: number } {
  const lo = axes.rangeFirstM - axes.rangeStepM / 2;
  return { lo: lo / 1_000, hi: (lo + axes.ranges * axes.rangeStepM) / 1_000 };
}

export function dopplerSpanHz(axes: PlotAxes): { lo: number; hi: number } {
  const lo = axes.dopplerFirstHz - axes.dopplerStepHz / 2;
  return { lo, hi: lo + axes.dopplers * axes.dopplerStepHz };
}

export function surfaceToPx(
  point: SurfacePoint,
  axes: PlotAxes,
  box: Box,
): { x: number; y: number } {
  const range = rangeSpanKm(axes);
  const doppler = dopplerSpanHz(axes);
  const across = range.hi - range.lo;
  const down = doppler.hi - doppler.lo;
  return {
    x: across > 0 ? ((point.rangeKm - range.lo) / across) * box.w : 0,
    y: down > 0 ? (1 - (point.dopplerHz - doppler.lo) / down) * box.h : 0,
  };
}

export function pxToSurface(x: number, y: number, axes: PlotAxes, box: Box): SurfacePoint {
  const range = rangeSpanKm(axes);
  const doppler = dopplerSpanHz(axes);
  return {
    rangeKm: range.lo + (box.w > 0 ? x / box.w : 0) * (range.hi - range.lo),
    dopplerHz: doppler.lo + (box.h > 0 ? 1 - y / box.h : 0) * (doppler.hi - doppler.lo),
  };
}

export function levelDb(level: number, axes: PlotAxes): number {
  return axes.dbMin + (level / 255) * (axes.dbMax - axes.dbMin);
}

function signed(value: number, digits: number): string {
  const text = value.toFixed(digits);
  return value > 0 && Number(text) !== 0 ? `+${text}` : text;
}

export function hoverText(point: SurfacePoint, carrierHz: number, db: number | null): string {
  const parts = [`${point.rangeKm.toFixed(1)} km`, `${signed(point.dopplerHz, 0)} Hz`];
  const speed = velocityMps(point.dopplerHz, carrierHz);
  if (speed !== null) {
    parts.push(`${signed(speed, 0)} m/s`);
  }
  if (db !== null) {
    parts.push(`${db.toFixed(0)} dB`);
  }
  return parts.join(" · ");
}
