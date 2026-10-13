import {
  allAutoTuning,
  autoTuning,
  tuneAllDelta,
  tuneDelta,
  tuningAllDelta,
  tuningDelta,
} from "../canvas/nodes/deviceNode";
import { leaveAuto } from "./autoOff";
import type { DeviceSet, Tuning } from "./types";
import { useDevicePatch } from "./useDevicePatch";

export interface Radio {
  node: string;
  set: DeviceSet;
}

export function useRadioTune(): {
  tuneRadio: (radio: Radio, stream: number, hz: number) => void;
  setTuning: (radio: Radio, stream: number, tuning: Tuning) => void;
  tuneAll: (radio: Radio, hz: number) => void;
  setTuningAll: (radio: Radio, tuning: Tuning) => void;
} {
  const { applyPatch, cachedSettings } = useDevicePatch();
  const fresh = ({ node, set }: Radio): DeviceSet => ({
    ...set,
    settings: cachedSettings(node) ?? set.settings,
  });
  const onAuto = (radio: Radio, stream: number): boolean => autoTuning(fresh(radio), stream);
  return {
    tuneRadio: (radio, stream, hz) =>
      leaveAuto(onAuto(radio, stream), () =>
        applyPatch(radio.node, tuneDelta(radio.set.capabilities, stream, hz)),
      ),
    setTuning: (radio, stream, tuning) =>
      leaveAuto(tuning === "manual" && onAuto(radio, stream), () =>
        applyPatch(radio.node, tuningDelta(radio.set.capabilities, stream, tuning)),
      ),
    tuneAll: (radio, hz) =>
      leaveAuto(allAutoTuning(fresh(radio)), () =>
        applyPatch(radio.node, tuneAllDelta(radio.set.capabilities, hz)),
      ),
    setTuningAll: (radio, tuning) =>
      leaveAuto(tuning === "manual" && allAutoTuning(fresh(radio)), () =>
        applyPatch(radio.node, tuningAllDelta(radio.set.capabilities, tuning)),
      ),
  };
}
