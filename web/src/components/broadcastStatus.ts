import labels from "../generated/labels.json";
import type { BroadcastStatus } from "../lib/types";

type Fact = readonly [string, string];

const SYSTEM_LABELS: Record<BroadcastStatus["system"], string> = labels.broadcast_system;

export function broadcastSystem(system: BroadcastStatus["system"]): string {
  return SYSTEM_LABELS[system];
}

export function broadcastFacts(status: BroadcastStatus): Fact[] {
  return present([
    ["System", broadcastSystem(status.system)],
    ["Mode", status.transmission_mode?.toUpperCase()],
    ["Frequency error", status.locked ? signedHz(status.frequency_error_hz) : undefined],
    ["Symbol rate", status.symbol_rate == null ? undefined : `${status.symbol_rate} Bd`],
    ["Superframe", superframe(status.superframe)],
    ["Ensemble ID", status.ensemble_id == null ? undefined : hex(status.ensemble_id)],
    ["Service ID", status.service_id == null ? undefined : hex(status.service_id)],
    ["Label", status.label],
    ["Audio frames", count(status.audio_frames_ok)],
    ["Audio failures", count(status.audio_frames_bad)],
    ["Audio error", status.audio_error],
    ["Video frames", count(status.video_frames_ok)],
    ["Video failures", count(status.video_frames_bad)],
    ["Video error", status.video_error],
    ["Data groups", count(status.data_groups_ok)],
    ["Data failures", count(status.data_groups_bad)],
    ["Data error", status.data_error],
    ["Dynamic label", status.dynamic_label],
  ]);
}

export function broadcastTitle(status: BroadcastStatus): string {
  return broadcastFacts(status)
    .map(([label, value]) => `${label}: ${value}`)
    .join("\n");
}

function superframe(frame: BroadcastStatus["superframe"]): string | undefined {
  if (frame == null) {
    return undefined;
  }
  const rows = [frame.sosf, frame.pilot, frame.trailer].filter((row) => row != null).join("/");
  return `format ${frame.format}, WH ${rows}, codes ${frame.reference}/${frame.payload}`;
}

function signedHz(hz: number): string {
  return `${hz >= 0 ? "+" : ""}${hz.toFixed(0)} Hz`;
}

function count(value: number | null | undefined): string | undefined {
  return value != null && value > 0 ? String(value) : undefined;
}

function hex(value: number): string {
  return `0x${value.toString(16).toUpperCase().padStart(4, "0")}`;
}

function present(rows: readonly (readonly [string, string | null | undefined])[]): Fact[] {
  return rows.flatMap(([label, value]) =>
    value == null || value === "" ? [] : [[label, value] as const],
  );
}
