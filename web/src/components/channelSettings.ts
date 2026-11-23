import { AUDIO, CHANNEL_FREQUENCY_SEED_HZ, IF_FILTER_LIMITS } from "../lib/limits";
import type {
  AudioProcessing,
  ChannelBand,
  ChannelDescriptor,
  ChannelParams,
  ChannelSettings,
  IfFilter,
  IfNotch,
  NotchSettings,
  Squelch,
} from "../lib/types";
import { formatSignedHz } from "./format";

export function settledParams(
  params: ChannelParams,
  defaults: ChannelParams | undefined,
): ChannelParams {
  if (defaults === undefined || defaults.type !== params.type) {
    return params;
  }
  const settled: Record<string, unknown> = { ...defaults.settings };
  for (const [name, value] of Object.entries(params.settings as Record<string, unknown>)) {
    if (value !== undefined) {
      settled[name] = value;
    }
  }
  return paramsWith(params.type, settled);
}

export function paramsWith(
  type: string,
  settings: Readonly<Record<string, unknown>>,
): ChannelParams {
  return { type, settings: settings };
}

export function channelFrequencyHz(settings: ChannelSettings): number {
  return settings.frequency_hz ?? CHANNEL_FREQUENCY_SEED_HZ;
}

export function mergeChannelSettings(
  current: ChannelSettings,
  edit: Partial<ChannelSettings>,
): ChannelSettings {
  return {
    frequency_hz: edit.frequency_hz ?? current.frequency_hz,
    squelch: edit.squelch ?? current.squelch ?? SQUELCH_OFF,
    params: edit.params ?? current.params,
    blanker: edit.blanker ?? current.blanker ?? {},
    if_filter: edit.if_filter ?? current.if_filter ?? {},
  };
}

export const IF_LIMITS = {
  maxNotches: IF_FILTER_LIMITS.notches,
  edgeHz: IF_FILTER_LIMITS.edge_hz,
  notchWidthHz: IF_FILTER_LIMITS.notch_width_hz,
  notchWidthSeedHz: IF_FILTER_LIMITS.notch_width_seed_hz,
} as const;

export type IfEdge = "low_hz" | "high_hz";

export function ownIfEdgeHz(edge: IfEdge, own: ChannelBand): number {
  return own[edge];
}

export function ifEdgeHz(filter: IfFilter, edge: IfEdge, own: ChannelBand): number {
  return filter[edge] ?? ownIfEdgeHz(edge, own);
}

/** An edge moved back onto the decoder's own is dropped, so the decoder keeps its filter. */
export function withIfEdge(filter: IfFilter, edge: IfEdge, hz: number, own: ChannelBand): IfFilter {
  return { ...filter, [edge]: hz === ownIfEdgeHz(edge, own) ? null : hz };
}

export function withIfNotchAdded(filter: IfFilter, atHz: number): IfFilter | null {
  const notches = filter.notches ?? [];
  if (notches.length >= IF_LIMITS.maxNotches) {
    return null;
  }
  return {
    ...filter,
    notches: [...notches, { frequency_hz: atHz, width_hz: IF_LIMITS.notchWidthSeedHz }],
  };
}

export function withIfNotchAt(filter: IfFilter, index: number, edit: Partial<IfNotch>): IfFilter {
  const notches = (filter.notches ?? []).map((notch, at) =>
    at === index ? { ...notch, ...edit } : notch,
  );
  return { ...filter, notches };
}

export function withIfNotchRemoved(filter: IfFilter, index: number): IfFilter {
  return { ...filter, notches: (filter.notches ?? []).filter((_, at) => at !== index) };
}

export interface ChannelPassband {
  lowHz: number;
  highHz: number;
  notches: readonly { atHz: number; widthHz: number }[];
}

/** What a channel decodes around its frequency: IF edges only narrow the decoder's own band. */
export function channelPassband(filter: IfFilter | undefined, band: ChannelBand): ChannelPassband {
  const own = { lowHz: band.low_hz, highHz: band.high_hz, notches: [] };
  if (!(filter?.enabled ?? false)) {
    return own;
  }
  return {
    lowHz: Math.max(own.lowHz, filter?.low_hz ?? own.lowHz),
    highHz: Math.min(own.highHz, filter?.high_hz ?? own.highHz),
    notches: (filter?.notches ?? []).map((notch) => ({
      atHz: notch.frequency_hz,
      widthHz: notch.width_hz ?? IF_LIMITS.notchWidthSeedHz,
    })),
  };
}

export function ifFilterSummary(filter: IfFilter, own: ChannelBand): string {
  const notches = filter.notches?.length ?? 0;
  const edged = (filter.low_hz ?? null) !== null || (filter.high_hz ?? null) !== null;
  if (!(filter.enabled ?? false)) {
    return "off";
  }
  if (!edged && notches === 0) {
    return "on";
  }
  const edges = (["low_hz", "high_hz"] as const).map((edge) =>
    formatSignedHz(ifEdgeHz(filter, edge, own)),
  );
  const parts = edged ? [edges.join(" ")] : [];
  if (notches > 0) {
    parts.push(`${notches} notch${notches === 1 ? "" : "es"}`);
  }
  return parts.join(", ");
}

export const SQUELCH_RANGE_DB = { min: -120, max: 0 } as const;

export const DEFAULT_SQUELCH_DB = -60;

export type SquelchMode = Squelch["mode"];

export const SQUELCH_OFF: Squelch = { mode: "off" };

export function squelchMode(squelch: Squelch | undefined): SquelchMode {
  return squelch?.mode ?? "off";
}

export function squelchLevelDb(squelch: Squelch | undefined): number | null {
  return squelch?.mode === "manual" ? squelch.level_db : null;
}

export function squelchMarginDb(squelch: Squelch | undefined): number | null {
  return squelch?.mode === "auto" ? squelch.margin_db : null;
}

export function squelchAt(mode: SquelchMode, held: { levelDb: number; marginDb: number }): Squelch {
  switch (mode) {
    case "off":
      return SQUELCH_OFF;
    case "manual":
      return { mode: "manual", level_db: held.levelDb };
    case "auto":
      return { mode: "auto", margin_db: held.marginDb };
  }
}

export function nudgedSquelch(squelch: Squelch | undefined, deltaDb: number): Squelch {
  if (squelch?.mode === "auto") {
    const { min, max } = AUDIO_LIMITS.squelchAutoMarginDb;
    return { mode: "auto", margin_db: Math.min(max, Math.max(min, squelch.margin_db + deltaDb)) };
  }
  const level = (squelchLevelDb(squelch) ?? DEFAULT_SQUELCH_DB) + deltaDb;
  return {
    mode: "manual",
    level_db: Math.min(SQUELCH_RANGE_DB.max, Math.max(SQUELCH_RANGE_DB.min, level)),
  };
}

export const AUDIO_DEFAULTS = {
  blankerThreshold: AUDIO.blanker_threshold_seed,
  clickThreshold: AUDIO.click_threshold_seed,
  squelchAutoMarginDb: AUDIO.squelch_auto_margin_seed_db,
  denoiseStrength: AUDIO.denoise_strength_seed,
  filterLowHz: AUDIO.filter_low_seed_hz,
  filterHighHz: AUDIO.filter_high_seed_hz,
  notchFreqHz: AUDIO.notch_freq_seed_hz,
  notchWidthHz: AUDIO.notch_width_seed_hz,
  peakFreqHz: AUDIO.peak_freq_seed_hz,
  peakWidthHz: AUDIO.peak_width_seed_hz,
  peakGainDb: AUDIO.peak_gain_seed_db,
} as const;

export const AUDIO_LIMITS = {
  maxNotches: AUDIO.notches,
  blankerThreshold: AUDIO.blanker_threshold,
  clickThreshold: AUDIO.click_threshold,
  squelchAutoMarginDb: AUDIO.squelch_auto_margin_db,
  toneHz: AUDIO.tone_hz,
  notchWidthHz: AUDIO.notch_width_hz,
  peakGainDb: AUDIO.peak_gain_db,
} as const;

export function mergeAudio(
  current: AudioProcessing,
  edit: Partial<AudioProcessing>,
): AudioProcessing {
  return { ...current, ...edit };
}

export function withNotchAdded(notches: NotchSettings[]): NotchSettings[] | null {
  if (notches.length >= AUDIO_LIMITS.maxNotches) {
    return null;
  }
  return [
    ...notches,
    { freq_hz: AUDIO_DEFAULTS.notchFreqHz, width_hz: AUDIO_DEFAULTS.notchWidthHz },
  ];
}

export function withNotchAt(
  notches: NotchSettings[],
  index: number,
  edit: Partial<NotchSettings>,
): NotchSettings[] {
  return notches.map((notch, at) => (at === index ? { ...notch, ...edit } : notch));
}

export function withNotchRemoved(notches: NotchSettings[], index: number): NotchSettings[] {
  return notches.filter((_, at) => at !== index);
}

export function audioChainActive(audio: AudioProcessing | undefined): boolean {
  if (audio === undefined) {
    return false;
  }
  return (
    (audio.click_removal?.enabled ?? false) ||
    (audio.filter?.enabled ?? false) ||
    (audio.notches?.length ?? 0) > 0 ||
    (audio.peak?.enabled ?? false) ||
    (audio.auto_notch ?? false) ||
    (audio.denoise?.enabled ?? false) ||
    (audio.agc ?? "off") !== "off"
  );
}

export function channelHasAudio(descriptor: ChannelDescriptor | undefined): boolean {
  return descriptor?.has_audio ?? true;
}

export function channelHasVideo(descriptor: ChannelDescriptor | undefined): boolean {
  return descriptor?.has_video ?? false;
}

export interface RadioWindow {
  lowHz: number;
  highHz: number;
}

/** The channel frequencies a radio can hear right now with the whole of `band` inside its span. */
export function radioWindowHz(
  centerHz: number | null | undefined,
  spanHz: number | null | undefined,
  band: ChannelBand | null,
): RadioWindow | null {
  if (spanHz == null || !Number.isFinite(spanHz) || spanHz <= 0) {
    return null;
  }
  if (centerHz == null || !Number.isFinite(centerHz)) {
    return null;
  }
  const lowHz = centerHz - spanHz / 2 - (band?.low_hz ?? 0);
  const highHz = centerHz + spanHz / 2 - (band?.high_hz ?? 0);
  return lowHz <= highHz ? { lowHz, highHz } : { lowHz: centerHz, highHz: centerHz };
}

export function reachesHz(frequencyHz: number, window: RadioWindow | null): boolean {
  return window === null || (frequencyHz >= window.lowHz && frequencyHz <= window.highHz);
}

export function bandWidthHz(band: ChannelBand | null): number | null {
  return band === null ? null : band.high_hz - band.low_hz;
}

export function clampOffsetHz(hz: number, limitHz: number | null): number {
  return limitHz === null ? hz : Math.min(limitHz, Math.max(-limitHz, hz));
}

export function offsetForFrequencyHz(
  frequencyHz: number,
  centerHz: number,
  limitHz: number | null,
): number | null {
  if (!Number.isFinite(frequencyHz) || !Number.isFinite(centerHz)) {
    return null;
  }
  const offsetHz = Math.round(frequencyHz - centerHz);
  return limitHz !== null && Math.abs(offsetHz) > limitHz ? null : offsetHz;
}
