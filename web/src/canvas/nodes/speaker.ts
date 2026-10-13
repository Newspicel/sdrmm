import { useEffect, useState, useSyncExternalStore } from "react";
import type { AudioTap } from "../../lib/audio/engine";
import { watchAudio } from "../../lib/audio/monitor";
import { audioEngine } from "../../lib/audio/useChannelAudio";
import type { Input } from "../../lib/resolved";

const LEVEL_TICK_MS = 100;
const FRAMES_PER_MS = 48;

export function pcmPeakDb(pcm: Float32Array): number {
  let peak = 0;
  for (const sample of pcm) {
    peak = Math.max(peak, Math.abs(sample));
  }
  return peak > 0 ? 20 * Math.log10(peak) : Number.NEGATIVE_INFINITY;
}

export function useAudioPeakDb(source: string, playing: boolean): number | undefined {
  const [peakDb, setPeakDb] = useState<number | undefined>(undefined);
  useEffect(() => {
    if (!playing) {
      return;
    }
    let held = Number.NEGATIVE_INFINITY;
    const stop = watchAudio(source, (pcm) => {
      held = Math.max(held, pcmPeakDb(pcm));
    });
    const timer = setInterval(() => {
      setPeakDb(held);
      held = Number.NEGATIVE_INFINITY;
    }, LEVEL_TICK_MS);
    return () => {
      stop();
      clearInterval(timer);
    };
  }, [source, playing]);
  return playing ? peakDb : undefined;
}

export interface SpeakerHealth {
  bufferedMs: number;
  trimmedMs: number;
  droppedMs: number;
  underruns: number;
  suspended: boolean;
}

function tapsOf(node: string, inputs: readonly Input[]): AudioTap[] {
  return inputs.map((input) => ({ node, port: "audio", source: input.source }));
}

function total(taps: readonly AudioTap[], read: (tap: AudioTap) => number): () => number {
  return () => taps.reduce((sum, tap) => sum + read(tap), 0);
}

export function useSpeakerHealth(
  node: string,
  inputs: readonly Input[],
  dev: boolean,
): SpeakerHealth {
  const subscribe = audioEngine.subscribe;
  const taps = tapsOf(node, inputs);
  const lostFrames = useSyncExternalStore(
    subscribe,
    total(taps, (tap) => audioEngine.getLostFrames(tap)),
  );
  const underruns = useSyncExternalStore(
    subscribe,
    total(taps, (tap) => audioEngine.getUnderruns(tap)),
  );
  const bufferedMs = useSyncExternalStore(
    subscribe,
    total(taps, (tap) => audioEngine.getBufferedMs(tap)),
  );
  const trimmedMs = useSyncExternalStore(
    subscribe,
    total(taps, (tap) => audioEngine.getTrimmedMs(tap)),
  );
  const suspended = useSyncExternalStore(subscribe, () =>
    taps.some((tap) => audioEngine.isSuspended(tap)),
  );
  return {
    bufferedMs: dev ? bufferedMs : 0,
    trimmedMs: dev ? trimmedMs : 0,
    droppedMs: lostFrames / FRAMES_PER_MS,
    underruns,
    suspended,
  };
}

export function speakerHealthShown(health: SpeakerHealth): boolean {
  return (
    health.suspended ||
    health.bufferedMs > 0 ||
    health.trimmedMs > 0 ||
    health.droppedMs > 0 ||
    health.underruns > 0
  );
}
