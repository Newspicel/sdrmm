import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { encodeMicrophonePcm, VoxGate } from "../../lib/microphone";
import { type Capture, startCapture } from "../../lib/microphoneCapture";
import { messageOf } from "../../lib/refusals";
import type { SdrSocket } from "../../lib/ws";

export interface MicrophoneOptions {
  vox: boolean;
  thresholdDb: number;
  hangMs: number;
  /** The talk key is held for this microphone. */
  held: boolean;
}

export interface Microphone {
  /** Keyed by hand: the button or the talk key. */
  keyed: boolean;
  /** Frames are going up the socket right now. */
  sending: boolean;
  /** Captured for the voice key and waiting for sound. */
  listening: boolean;
  level: number;
  error: string | null;
  key: () => void;
  unkey: () => void;
}

/**
 * Captures the microphone while keyed or while the voice key listens, and sends each frame up
 * the socket under `node` while keyed or while the voice key is open.
 */
export function useMicrophone(
  node: string,
  socket: SdrSocket,
  options: MicrophoneOptions,
): Microphone {
  const [pressed, setPressed] = useState(false);
  const [sending, setSending] = useState(false);
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const keyed = pressed || options.held;
  const capturing = keyed || options.vox;
  const live = useRef({ keyed, vox: options.vox });
  const gate = useRef(new VoxGate(options.thresholdDb, options.hangMs));

  useLayoutEffect(() => {
    live.current = { keyed, vox: options.vox };
  });

  useEffect(() => {
    gate.current = new VoxGate(options.thresholdDb, options.hangMs);
  }, [options.thresholdDb, options.hangMs]);

  useEffect(() => {
    if (!capturing) {
      return;
    }
    let stopped = false;
    let capture: Capture | null = null;
    startCapture((frame, peak) => {
      const voiced = gate.current.feed(peak, performance.now());
      const open = live.current.keyed || (live.current.vox && voiced);
      if (open) {
        socket.send({ type: "Speak", data: { node, pcm: encodeMicrophonePcm(frame) } });
      }
      setSending(open);
      setLevel(peak);
    }).then(
      (started) => {
        if (stopped) {
          started.stop();
        } else {
          capture = started;
        }
      },
      (failure: unknown) => {
        if (!stopped) {
          setError(messageOf(failure));
          setPressed(false);
        }
      },
    );
    return () => {
      stopped = true;
      capture?.stop();
      setSending(false);
      setLevel(0);
    };
  }, [capturing, node, socket]);

  const key = useCallback(() => {
    setError(null);
    setPressed(true);
  }, []);
  const unkey = useCallback(() => setPressed(false), []);

  return {
    keyed,
    sending,
    listening: capturing && !sending && error === null,
    level,
    error,
    key,
    unkey,
  };
}
