import labels from "../../generated/labels.json" with { type: "json" };
import { GATE_TEXT } from "../../lib/arrays";
import type { RangeDopplerFrame } from "../../lib/frame";
import { LIGHT_SPEED_M_S, RADAR_LIMITS } from "../../lib/limits";
import type {
  PassiveRadarParams,
  ProcessorGate,
  RadarAoa,
  RadarAxes,
  RadarProblem,
  RadarTrack,
  RadarUpdate,
} from "../../lib/types";
import type { Chip } from "./ProcessorHealth";

export const TRACK_ROWS = 6;
export const STALE_SLACK_MS = 1_000;
export const NO_TX = "no tx";
export const RADAR_TX_PORT = "tx";

type IlluminatorKind = PassiveRadarParams["illuminator"]["kind"];

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

export function readingAxes(axes: RadarAxes): PlotAxes {
  return {
    rangeFirstM: 0,
    rangeStepM: axes.range_step_m,
    ranges: axes.gates,
    dopplerFirstHz: (-(axes.doppler_rows - 1) / 2) * axes.doppler_step_hz,
    dopplerStepHz: axes.doppler_step_hz,
    dopplers: axes.doppler_rows,
    carrierHz: axes.carrier_hz,
    dbMin: RADAR_LIMITS.surface_db.min,
    dbMax: RADAR_LIMITS.surface_db.max,
  };
}

export function plotAxes(
  frame: RangeDopplerFrame | null,
  update: RadarUpdate | null,
): PlotAxes | null {
  if (frame !== null) {
    return frameAxes(frame);
  }
  if (update !== null && update.axes.gates > 0 && update.axes.doppler_rows > 0) {
    return readingAxes(update.axes);
  }
  return null;
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

export function sortTracks(tracks: readonly RadarTrack[], limit: number): RadarTrack[] {
  return tracks.toSorted((a, b) => b.snr_db - a.snr_db || a.id - b.id).slice(0, limit);
}

export function isStale(receivedAt: number, update: RadarUpdate, now: number): boolean {
  return now - receivedAt > 3 * update.axes.hop_ms + STALE_SLACK_MS;
}

const PROBLEM_TEXT: Readonly<Record<RadarProblem["kind"], string>> = labels.radar_problem;

export function problemLabel(problem: RadarProblem): string {
  return problem.kind === "refused" ? problem.detail : PROBLEM_TEXT[problem.kind];
}

const CHIP_TITLE: Partial<Readonly<Record<RadarProblem["kind"], string>>> = {
  table_out_of_range: "Bearings use the ideal geometry",
};

export function radarChips(update: RadarUpdate | null): Chip[] {
  return (update?.problems ?? []).flatMap((problem) => {
    const title = CHIP_TITLE[problem.kind];
    return title === undefined ? [] : [{ label: problemLabel(problem), title }];
  });
}

export const ILLUMINATOR_TEXT: Readonly<Record<IlluminatorKind, string>> = {
  fm: "FM",
  dab: "DAB",
  dvbt_partial: "DVB-T",
  custom: "Custom",
};

export interface Subtitle {
  text: string;
  warn: boolean;
}

export interface RadarState {
  wired: boolean;
  txWired: boolean;
  gate: ProcessorGate | null;
  update: RadarUpdate | null;
  stale: boolean;
  illuminator: IlluminatorKind;
  lanes: number;
}

export function radarSubtitle(state: RadarState): Subtitle {
  if (!state.wired) {
    return { text: "no array", warn: false };
  }
  if (state.gate !== null) {
    return { text: GATE_TEXT[state.gate].toLowerCase(), warn: true };
  }
  if (!state.txWired) {
    return { text: NO_TX, warn: true };
  }
  const problem = state.update?.problems?.find((held) => CHIP_TITLE[held.kind] === undefined);
  if (problem !== undefined) {
    return { text: problemLabel(problem), warn: true };
  }
  if (state.stale) {
    return { text: "stale", warn: true };
  }
  if (state.update !== null) {
    const mhz = (state.update.axes.carrier_hz / 1e6).toFixed(2);
    return { text: `${ILLUMINATOR_TEXT[state.illuminator]} ${mhz} MHz`, warn: false };
  }
  return { text: `${state.lanes} ${state.lanes === 1 ? "lane" : "lanes"}`, warn: false };
}

export interface BearingCell {
  text: string;
  arrayFrame: boolean;
}

export function bearingCell(aoa: RadarAoa | null | undefined): BearingCell {
  if (aoa == null) {
    return { text: "-", arrayFrame: false };
  }
  if (aoa.bearing_deg != null) {
    return { text: `${Math.round(aoa.bearing_deg)}°`, arrayFrame: false };
  }
  return { text: `${Math.round(aoa.azimuth_deg)}°`, arrayFrame: true };
}

export function truthCell(track: RadarTrack): string {
  return track.truth?.callsign ?? track.truth?.id ?? "-";
}
