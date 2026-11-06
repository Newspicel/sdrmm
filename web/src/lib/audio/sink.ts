import { registerMediaLatency } from "../mediaLatency";
import { producerOf } from "../ports";
import type { OpusPacketDecoder } from "./decoder";
import { createOpusPacketDecoder } from "./decoder";
import type { AudioSink, SinkFactory } from "./engine";
import { isWatched, publishAudio } from "./monitor";
import { outputIdOf } from "./outputs";
import { createPlayback, type Playback } from "./playback";
import type { WorkletReport } from "./worklet";
import { CHANNELS, SAMPLE_RATE, TARGET_FRAMES } from "./worklet";

const VOLUME_RANGE_DB = 60;
const VOLUME_RAMP_SECONDS = 0.02;
const DEVICE_SETTLE_MS = 750;

interface Output {
  readonly key: string;
  readonly context: AudioContext;
  sinks: number;
}

type SinkedOptions = AudioContextOptions & { sinkId?: string };

const SYSTEM = "";

const outputs = new Map<string, Output>();
const outputListeners = new Set<(running: boolean) => void>();
const rerouteListeners = new Set<() => void>();
let recoveryArmed = false;
let watchingDevices = false;
let rerouteTimer: ReturnType<typeof setTimeout> | undefined;

export function gainForVolume(volume: number): number {
  const position = Math.min(1, Math.max(0, volume));
  return position <= 0 ? 0 : 10 ** ((position - 1) * (VOLUME_RANGE_DB / 20));
}

export function isOutputRunning(): boolean {
  return [...outputs.values()].every(running);
}

function running({ context }: Output): boolean {
  return context.state === "running";
}

export function onOutputStateChange(listener: (running: boolean) => void): () => void {
  outputListeners.add(listener);
  return () => outputListeners.delete(listener);
}

export function onOutputRerouted(listener: () => void): () => void {
  rerouteListeners.add(listener);
  return () => rerouteListeners.delete(listener);
}

export function resumeAudioOutput(): void {
  attemptResume();
}

function attemptResume(): void {
  for (const { context } of outputs.values()) {
    if (context.state !== "running" && context.state !== "closed") {
      context.resume().catch(() => {});
    }
  }
}

function handleStateChange(): void {
  const allRunning = isOutputRunning();
  if (allRunning) {
    disarmRecovery();
  } else {
    armRecovery();
  }
  for (const listener of outputListeners) {
    listener(allRunning);
  }
  for (const output of outputs.values()) {
    if (output.sinks === 0 && running(output)) {
      closeOutput(output.key);
    }
  }
}

async function acquireOutput(name: string | null): Promise<Output> {
  const key = name ?? SYSTEM;
  const held = outputs.get(key);
  if (held !== undefined) {
    held.sinks += 1;
    return held;
  }
  const options: SinkedOptions = { sampleRate: SAMPLE_RATE };
  if (name !== null) {
    options.sinkId = await outputIdOf(name);
  }
  const raced = outputs.get(key);
  if (raced !== undefined) {
    raced.sinks += 1;
    return raced;
  }
  const context = new AudioContext(options);
  context.addEventListener("statechange", handleStateChange);
  watchOutputDevices();
  const output = { key, context, sinks: 1 };
  outputs.set(key, output);
  return output;
}

function releaseOutput(output: Output): void {
  if (outputs.get(output.key) !== output) {
    return;
  }
  output.sinks -= 1;
  if (output.sinks === 0 && running(output)) {
    closeOutput(output.key);
  }
}

function closeOutput(key: string): void {
  const output = outputs.get(key);
  if (output === undefined) {
    return;
  }
  outputs.delete(key);
  output.context.removeEventListener("statechange", handleStateChange);
  if (outputs.size === 0) {
    disarmRecovery();
  }
  output.context.close().catch(() => {});
}

function watchOutputDevices(): void {
  const devices = globalThis.navigator?.mediaDevices;
  if (watchingDevices || devices === undefined) {
    return;
  }
  watchingDevices = true;
  devices.addEventListener("devicechange", () => {
    clearTimeout(rerouteTimer);
    rerouteTimer = setTimeout(reroute, DEVICE_SETTLE_MS);
  });
}

function reroute(): void {
  if (![...outputs.values()].some(running)) {
    return;
  }
  for (const key of outputs.keys()) {
    closeOutput(key);
  }
  for (const listener of rerouteListeners) {
    listener();
  }
}

function armRecovery(): void {
  if (recoveryArmed) {
    return;
  }
  recoveryArmed = true;
  document.addEventListener("pointerdown", attemptResume, true);
  document.addEventListener("visibilitychange", handleVisibility);
}

function disarmRecovery(): void {
  if (!recoveryArmed) {
    return;
  }
  recoveryArmed = false;
  document.removeEventListener("pointerdown", attemptResume, true);
  document.removeEventListener("visibilitychange", handleVisibility);
}

function handleVisibility(): void {
  if (document.visibilityState === "visible") {
    attemptResume();
  }
}

export const createWebAudioSink: SinkFactory = async (
  key,
  volume,
  onError,
  onReport,
  name = null,
) => {
  const output = await acquireOutput(name);
  const { context } = output;
  if (context.state !== "running") {
    attemptResume();
    handleStateChange();
  }
  let lastReport: WorkletReport = { underruns: 0 };
  let decoderDroppedFrames = 0;
  const onDropped = (frames: number): void => {
    decoderDroppedFrames += frames;
    onReport({ ...lastReport, decoderDroppedFrames });
  };
  let playback: Playback;
  try {
    playback = await createPlayback(
      context,
      (report) => {
        lastReport = report;
        onReport({ ...lastReport, decoderDroppedFrames });
      },
      onError,
    );
  } catch (err) {
    releaseOutput(output);
    throw err;
  }
  const gain = new GainNode(context, { gain: gainForVolume(volume) });
  playback.node.connect(gain).connect(context.destination);

  let closed = false;
  let received = false;
  const removeLatency = registerMediaLatency(producerOf(key), () => {
    if (!received || closed || context.state !== "running") return 0;
    const frames = lastReport.bufferedFrames ?? TARGET_FRAMES;
    return (
      1000 * (frames / SAMPLE_RATE + (context.baseLatency ?? 0) + (context.outputLatency ?? 0))
    );
  });
  let monitored = new Float32Array(0);
  const emit = (pcm: Float32Array): void => {
    if (closed) {
      return;
    }
    const watched = isWatched(key);
    const samples = pcm.length;
    if (watched) {
      if (monitored.length < samples) {
        monitored = new Float32Array(samples);
      }
      monitored.set(pcm);
    }
    received = true;
    playback.send(pcm);
    if (watched) {
      publishAudio(key, monitored.subarray(0, samples), CHANNELS);
    }
  };

  let decoder: OpusPacketDecoder;
  try {
    decoder = await createOpusPacketDecoder(CHANNELS, emit, onError, onDropped);
  } catch (err) {
    playback.send("close");
    playback.release();
    gain.disconnect();
    removeLatency();
    releaseOutput(output);
    throw err;
  }

  const sink: AudioSink = {
    ready: playback.ready,
    push(opus, timestampUs, channels) {
      if (closed) {
        return false;
      }
      if (channels !== 1 && channels !== 2) {
        onError(new Error(`Unsupported audio channel count: ${channels}`));
        return false;
      }
      return decoder.decode(opus, timestampUs);
    },
    conceal(frames) {
      playback.send(new Float32Array(frames * CHANNELS));
    },
    setVolume(v) {
      gain.gain.setTargetAtTime(gainForVolume(v), context.currentTime, VOLUME_RAMP_SECONDS);
    },
    reset() {
      received = false;
      decoder.reset();
      playback.send("reset");
    },
    close() {
      if (closed) {
        return;
      }
      closed = true;
      removeLatency();
      decoder.close();
      playback.send("close");
      playback.release();
      gain.disconnect();
      releaseOutput(output);
    },
  };
  return sink;
};
