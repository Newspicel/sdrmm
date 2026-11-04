import type { ChannelHealth } from "../../lib/types";
import { formatSignedHz } from "../format";
import type { ChipTone } from "./Chips";
import { Stat } from "./Stats";

export interface HealthStat {
  key: string;
  label: string;
  text: string;
  title: string;
  tone?: ChipTone;
}

/** The footer stats a channel's health carries, one per value the decoder set. */
export function healthStats(health: ChannelHealth | undefined): HealthStat[] {
  if (health === undefined) {
    return [];
  }
  const stats: HealthStat[] = [];
  if (health.snr_db != null) {
    stats.push({
      key: "snr",
      label: "SNR",
      text: `${health.snr_db.toFixed(1)} dB`,
      title: "Signal to noise ratio",
    });
  }
  if (health.freq_error_hz != null) {
    stats.push({
      key: "freq",
      label: "Δf",
      text: formatSignedHz(health.freq_error_hz),
      title: "Offset from the tuned frequency",
    });
  }
  if (health.errors_per_s != null) {
    stats.push({
      key: "errors",
      label: "Err",
      text: `${health.errors_per_s.toFixed(1)}/s`,
      title: "Frames failing their check, per second",
      ...(health.errors_per_s > 0 ? { tone: "warn" as const } : {}),
    });
  }
  return stats;
}

export function hasHealth(health: ChannelHealth | undefined): boolean {
  return health?.lock != null || healthStats(health).length > 0;
}

/** The standard health footer: a lock dot, then each value the decoder set. */
export function HealthStats({ health }: { health: ChannelHealth | undefined }) {
  return (
    <>
      {health?.lock != null && <LockDot locked={health.lock} />}
      {healthStats(health).map((stat) => (
        <Stat key={stat.key} label={stat.label} title={stat.title} tone={stat.tone}>
          {stat.text}
        </Stat>
      ))}
    </>
  );
}

function LockDot({ locked }: { locked: boolean }) {
  const said = locked ? "Locked" : "No lock";
  return (
    <span
      role="img"
      aria-label={said}
      title={said}
      className={`size-2 shrink-0 rounded-full ${locked ? "bg-ok" : "bg-warn"}`}
    />
  );
}
