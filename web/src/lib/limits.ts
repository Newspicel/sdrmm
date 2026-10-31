import limits from "../generated/limits.json" with { type: "json" };

export interface Bounds {
  min: number;
  max: number;
  above?: boolean;
}

export interface BandLimits {
  offset_hz: Bounds;
  bandwidth_hz: Bounds;
}

export const LIGHT_SPEED_M_S = limits.light_speed_m_s;
export const BAND_SEED_HZ = limits.band_seed_hz;
export const SPECTRUM_SIGNAL_MARGIN_DB = limits.spectrum_signal_margin_db;
export const SONDE_BURST_M = limits.sonde_burst_m;
export const ARRAY_LIMITS = limits.array;
export const FUSION_LIMITS = limits.fusion;
export const RADAR_LIMITS = limits.radar;
export const STITCH_LIMITS = limits.stitch;
export const DF_LIMITS = limits.df;
export const BEAMFORMER_LIMITS = limits.beamformer;
export const SPATIAL_LIMITS = limits.spatial;
export const CORRELATOR_LIMITS = limits.correlator;
export const POLARIMETER_LIMITS = limits.polarimeter;
export const HUNT_LIMITS = limits.hunt;
export const PRESENCE_LIMITS = limits.presence;
export const ROTATOR_LIMITS = limits.rotator;
export const SCHEDULE_LIMITS = limits.schedule;
export const MAX_NODES = limits.patch.nodes;
export const MAX_EDGES = limits.patch.wires;
export const MAX_STREAMS = limits.patch.streams;
export const MAX_SCAN_TARGETS = limits.scan.targets;
export const MAX_NAME_LEN = limits.name_len;
export const MAX_NOTE_LEN = limits.note_len;
export const MAX_FILTER_KINDS = limits.filter.kinds;
export const MAX_FILTER_IDS = limits.filter.ids;
export const MAX_FILTER_DURATION_MS = limits.filter.duration_ms;
export const MAX_FILTER_TEXT_LEN = limits.filter.text_len;
export const CHANNEL_FREQUENCY_SEED_HZ = limits.channel_frequency_seed_hz;
export const AUDIO = limits.audio;
export const AUDIO_RATE_HZ = limits.audio.rate_hz;
export const SURVEY_LIMITS = limits.survey;
export const WMBUS_LIMITS = limits.wmbus;
export const WORKSPACE_STATE_VERSION = limits.workspace_state_version;

export function lowest(bounds: Bounds, step: number): number {
  return bounds.above === true ? bounds.min + step : bounds.min;
}

export function scaled(bounds: Bounds, factor: number): Bounds {
  return { ...bounds, min: bounds.min * factor, max: bounds.max * factor };
}

export function holds(bounds: Bounds, value: number): boolean {
  const low = bounds.above === true ? value > bounds.min : value >= bounds.min;
  return low && value <= bounds.max;
}

export function powersOfTwo(bounds: Bounds): number[] {
  const powers: number[] = [];
  for (let value = 1; value <= bounds.max; value *= 2) {
    if (holds(bounds, value)) {
      powers.push(value);
    }
  }
  return powers;
}
