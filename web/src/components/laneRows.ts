import { rxStreamCount } from "../canvas/graph";
import type { Capabilities, DeviceSet, DeviceSettings, GainStage } from "../lib/types";
import { forStream } from "../lib/useDevicePatch";
import { agcStageIndex, isSwitch } from "./capabilities";
import type { Options } from "./controls";

export interface LaneLayout {
  lanes: number;
  perLane: boolean;
  master: boolean;
  inputs: boolean;
}

export function laneLayout(caps: Capabilities): LaneLayout {
  const streams = rxStreamCount(caps);
  const perLane = caps.per_stream?.gain === true && streams > 1;
  const inputs = (caps.rx_inputs?.length ?? 0) > 1;
  return { lanes: perLane ? streams : 1, perLane, master: perLane, inputs };
}

export function rxStages(caps: Capabilities): GainStage[] {
  return caps.gains.filter((stage) => stage.kind !== "tx");
}

export function txStages(caps: Capabilities): GainStage[] {
  return caps.gains.filter((stage) => stage.kind === "tx");
}

export function meterStage(caps: Capabilities): GainStage | undefined {
  const stages = rxStages(caps);
  const driven = stages[agcStageIndex(stages)];
  return driven !== undefined && !isSwitch(driven) ? driven : stages.find((s) => !isSwitch(s));
}

export function laneGainDb(
  settings: DeviceSettings,
  caps: Capabilities,
  stream: number,
  stage: GainStage,
): number {
  const lane = forStream(settings, stream, caps.per_stream);
  return lane.gains?.find((gain) => gain.stage === stage.name)?.value_db ?? stage.range.min;
}

export function laneGains(set: DeviceSet, stage: GainStage): number[] {
  const { lanes } = laneLayout(set.capabilities);
  return Array.from({ length: lanes }, (_, stream) =>
    laneGainDb(set.settings, set.capabilities, stream, stage),
  );
}

export function spreadOf(values: readonly number[]): { uniform: boolean; mean: number } {
  const first = values[0] ?? 0;
  const mean = values.length === 0 ? 0 : values.reduce((a, b) => a + b, 0) / values.length;
  return { uniform: values.every((value) => value === first), mean };
}

export function allLanesGain(
  caps: Capabilities,
  stage: GainStage,
  value_db: number,
): DeviceSettings {
  const { lanes, perLane } = laneLayout(caps);
  if (!perLane) {
    return { gains: [{ stage: stage.name, value_db }] };
  }
  return {
    streams: Array.from({ length: lanes }, (_, stream) => ({
      stream,
      gains: [{ stage: stage.name, value_db }],
    })),
  };
}

export function laneGain(
  caps: Capabilities,
  stream: number,
  stage: GainStage,
  value_db: number,
): DeviceSettings {
  if (!laneLayout(caps).perLane) {
    return { gains: [{ stage: stage.name, value_db }] };
  }
  return { streams: [{ stream, gains: [{ stage: stage.name, value_db }] }] };
}

export function inputOptions(caps: Capabilities): Options<number> {
  return (caps.rx_inputs ?? []).map((name, input) => ({ value: input, label: name }));
}

export function pickedInputs(settings: DeviceSettings): number[] {
  return settings.rx_inputs ?? [0];
}

export function laneInputName(set: DeviceSet, stream: number): string | undefined {
  const input = set.settings.rx_inputs?.[stream];
  return input === undefined ? undefined : set.capabilities.rx_inputs?.[input];
}
