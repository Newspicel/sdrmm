import type {
  BandChannel,
  BandPlan,
  DeviceSet,
  ScannerStatus,
  ScanRange,
  ScanSettings,
} from "../lib/types";
import { formatMhz as fixedWidthMhz } from "./format";

export const MIN_STEP_KHZ = 0.1;

export interface ScanPreset {
  id: string;
  name: string;
  ranges: ScanRange[];
  frequencies: number[];
  channels: BandChannel[];
}

export function rangeProblem(settings: ScanSettings): string | null {
  const ranges = settings.ranges ?? [];
  if (ranges.length === 0 && (settings.frequencies ?? []).length === 0) {
    return "add a range";
  }
  const reversed = ranges.findIndex((range) => range.stop_hz < range.start_hz);
  if (reversed < 0) {
    return null;
  }
  const line = ranges.length > 1 ? `range ${reversed + 1}: ` : "";
  return `${line}the stop frequency is below the start`;
}

export function targetCount(settings: ScanSettings): number {
  const swept = (settings.ranges ?? []).reduce(
    (total, r) => total + Math.max(0, Math.floor((r.stop_hz - r.start_hz) / r.step_hz) + 1),
    0,
  );
  return swept + (settings.frequencies ?? []).length;
}

export function withoutHz(list: readonly number[] | undefined, hz: number): number[] {
  return (list ?? []).filter((entry) => entry !== hz);
}

export function withHz(list: readonly number[] | undefined, hz: number): number[] {
  const current = list ?? [];
  return current.includes(hz) ? [...current] : [...current, hz].toSorted((a, b) => a - b);
}

function presetOf(name: string, start: number, stop: number, step: number | null | undefined) {
  return { name, start, stop, step: step ?? null, channels: new Map<number, BandChannel>() };
}

export function scanPresets(plan: BandPlan | null): ScanPreset[] {
  const byName = new Map<string, ReturnType<typeof presetOf>>();
  for (const allocation of plan?.allocations ?? []) {
    const channels = allocation.channels ?? [];
    const step = allocation.channel_step_hz;
    const tunable = channels.length > 0 || (step != null && step > 0);
    if ((allocation.aliases ?? []).length === 0 || !tunable) {
      continue;
    }
    const preset =
      byName.get(allocation.name) ??
      presetOf(allocation.name, allocation.start_hz, allocation.stop_hz, step);
    preset.start = Math.min(preset.start, allocation.start_hz);
    preset.stop = Math.max(preset.stop, allocation.stop_hz);
    for (const channel of channels) {
      preset.channels.set(channel.hz, channel);
    }
    byName.set(allocation.name, preset);
  }
  return [...byName.values()]
    .toSorted((a, b) => a.start - b.start)
    .map((preset) => {
      const channels = [...preset.channels.values()].toSorted((a, b) => a.hz - b.hz);
      const listed = channels.length > 0;
      return {
        id: `${preset.name}:${preset.start}`,
        name: preset.name,
        channels,
        frequencies: listed ? channels.map((channel) => channel.hz) : [],
        ranges:
          listed || preset.step === null
            ? []
            : [{ start_hz: preset.start, stop_hz: preset.stop, step_hz: preset.step }],
      };
    });
}

export function applyPreset(settings: ScanSettings, preset: ScanPreset): ScanSettings {
  return { ...settings, ranges: preset.ranges, frequencies: preset.frequencies };
}

function same(a: readonly unknown[], b: readonly unknown[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function presetIn(
  settings: ScanSettings,
  presets: readonly ScanPreset[],
): ScanPreset | null {
  return (
    presets.find(
      (preset) =>
        same(preset.ranges, settings.ranges ?? []) &&
        same(preset.frequencies, settings.frequencies ?? []),
    ) ?? null
  );
}

export function channelName(plan: BandPlan | null, hz: number): string | null {
  for (const allocation of plan?.allocations ?? []) {
    const channel = (allocation.channels ?? []).find((entry) => Math.abs(entry.hz - hz) < 1);
    if (channel !== undefined) {
      return channel.name;
    }
  }
  return null;
}

export function stateLabel(status: ScannerStatus): string {
  if (status.state === "holding") {
    return status.settings.mode === "all" ? "visiting" : "holding";
  }
  return status.state === "done" ? "done" : "scanning";
}

export function liveStatus(
  set: DeviceSet | null,
  channel: number | null,
  pushed: ScannerStatus | undefined,
): ScannerStatus | null {
  const listed = set?.scanners?.find((scanner) => scanner.settings.channel === channel);
  if (listed === undefined) {
    return null;
  }
  return pushed ?? listed;
}

export function sweepKind(set: DeviceSet | null, status: ScannerStatus | null): string {
  if (status !== null) {
    return status.hardware_sweep === true ? "the radio's own" : "by retuning";
  }
  return set?.capabilities.hardware_sweep === true ? "the radio's own" : "by retuning";
}

export function formatMhz(hz: number | null | undefined): string {
  return hz == null || !Number.isFinite(hz) ? "-" : fixedWidthMhz(hz);
}

export function formatDb(db: number | null | undefined): string {
  return db == null || !Number.isFinite(db) ? "-" : `${db.toFixed(1)} dB`;
}
