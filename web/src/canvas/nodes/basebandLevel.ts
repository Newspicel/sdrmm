import { useEffect, useState } from "react";
import { iqHub } from "../../lib/portTaps";
import type { ChannelLevel } from "../../lib/types";

const PORT = "baseband";
const REFRESH_MS = 100;
const STALE_MS = 1_000;
const PEAK_FALL_DB = 0.5;

/** Mean and peak power of interleaved I/Q samples, in dB below full scale. */
export function iqPowerDb(samples: Float32Array): { meanDb: number; peakDb: number } {
  let sum = 0;
  let peak = 0;
  for (let index = 0; index + 1 < samples.length; index += 2) {
    const i = samples[index] ?? 0;
    const q = samples[index + 1] ?? 0;
    const power = i * i + q * q;
    sum += power;
    peak = Math.max(peak, power);
  }
  const count = Math.floor(samples.length / 2);
  return {
    meanDb: count === 0 ? Number.NEGATIVE_INFINITY : 10 * Math.log10(sum / count),
    peakDb: 10 * Math.log10(peak),
  };
}

/** The level of what a node sends on its baseband output, while it sends. */
export function useBasebandLevel(node: string): ChannelLevel | undefined {
  const [level, setLevel] = useState<ChannelLevel | undefined>(undefined);
  useEffect(() => {
    let latest: ChannelLevel | undefined;
    let heard = 0;
    const unsubscribe = iqHub.subscribe(node, PORT, (frame) => {
      const { meanDb, peakDb } = iqPowerDb(frame.samples);
      const held = latest === undefined ? peakDb : Math.max(peakDb, latest.peak_db - PEAK_FALL_DB);
      latest = { channel: 0, level_db: meanDb, peak_db: held };
      heard = performance.now();
    });
    const timer = setInterval(() => {
      const fresh = latest !== undefined && performance.now() - heard < STALE_MS;
      setLevel(fresh ? latest : undefined);
    }, REFRESH_MS);
    return () => {
      unsubscribe();
      clearInterval(timer);
    };
  }, [node]);
  return level;
}
