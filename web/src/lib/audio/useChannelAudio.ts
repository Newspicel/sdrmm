import { useCallback, useEffect, useSyncExternalStore } from "react";
import type { SdrSocket } from "../ws";
import { AudioEngine, type AudioTap } from "./engine";
import {
  createWebAudioSink,
  onOutputRerouted,
  onOutputStateChange,
  resumeAudioOutput,
} from "./sink";

export interface ChannelAudio {
  playing: boolean;
  pending: boolean;
  suspended: boolean;
  error: string | null;
  start: () => void;
  stop: () => void;
  dismissError: () => void;
  resumeOutput: () => void;
  volume: number;
  setVolume: (v: number) => void;
  lostFrames: number;
  underruns: number;
  bufferedMs?: number;
  trimmedMs?: number;
}

export const audioEngine = new AudioEngine(createWebAudioSink);
onOutputStateChange((running) => audioEngine.setOutputRunning(running));
onOutputRerouted(() => audioEngine.rerouteOutput());

function useTapValue<T>(tap: AudioTap, get: (tap: AudioTap) => T): T {
  return useSyncExternalStore(audioEngine.subscribe, () => get(tap));
}

export function useChannelAudio(socket: SdrSocket | null, tap: AudioTap): ChannelAudio {
  useEffect(() => {
    if (socket) {
      audioEngine.attach(socket);
    }
  }, [socket]);

  const { node, port, source } = tap;
  const playing = useTapValue(tap, (at) => audioEngine.isPlaying(at));
  const pending = useTapValue(tap, (at) => audioEngine.isPending(at));
  const suspended = useTapValue(tap, (at) => audioEngine.isSuspended(at));
  const error = useTapValue(tap, (at) => audioEngine.getError(at));
  const volume = useTapValue(tap, (at) => audioEngine.getVolume(at));
  const lostFrames = useTapValue(tap, (at) => audioEngine.getLostFrames(at));
  const underruns = useTapValue(tap, (at) => audioEngine.getUnderruns(at));
  const bufferedMs = useTapValue(tap, (at) => audioEngine.getBufferedMs(at));
  const trimmedMs = useTapValue(tap, (at) => audioEngine.getTrimmedMs(at));

  const start = useCallback(() => {
    if (!socket) {
      return;
    }
    audioEngine.attach(socket);
    audioEngine.start({ node, port, source });
  }, [socket, node, port, source]);

  const stop = useCallback(() => {
    audioEngine.stop({ node, port, source });
  }, [node, port, source]);

  const dismissError = useCallback(() => {
    audioEngine.clearError({ node, port, source });
  }, [node, port, source]);

  const setVolume = useCallback(
    (v: number) => {
      audioEngine.setVolume({ node, port, source }, v);
    },
    [node, port, source],
  );

  return {
    playing,
    pending,
    suspended,
    error,
    start,
    stop,
    dismissError,
    resumeOutput: resumeAudioOutput,
    volume,
    setVolume,
    lostFrames,
    underruns,
    bufferedMs,
    trimmedMs,
  };
}
