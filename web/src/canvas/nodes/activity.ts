import type { RadioActivity } from "../../lib/types";
import type { LiveSignal } from "./NodeShell";

const SIGNAL: Record<RadioActivity, LiveSignal | null> = {
  idle: null,
  receiving: { tone: "ok", label: "Receiving" },
  sending: { tone: "danger", label: "Sending" },
  receive_paused: { tone: "warn", label: "Sending, receive paused" },
};

/** What a radio does now, on its header dot, so a crowded header never drops it. */
export function activitySignal(activity: RadioActivity): LiveSignal | null {
  return SIGNAL[activity];
}
