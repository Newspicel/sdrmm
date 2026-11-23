import { useCallback, useEffect, useState } from "react";
import { encodeMicrophonePcm } from "../../lib/microphone";
import { type Capture, startCapture } from "../../lib/microphoneCapture";
import { messageOf } from "../../lib/refusals";
import type { SendMode } from "../../lib/types";
import type { SdrSocket } from "../../lib/ws";

export interface MicrophoneOptions {
  send: SendMode;
  /** The microphone's audio reaches a listener: an armed transmitter, a speaker, a chat. */
  heard: boolean;
  /** The talk key is held for this microphone. */
  held: boolean;
}

export interface Microphone {
  /** Keyed by hand: the button or the talk key. */
  keyed: boolean;
  /** The browser microphone is open and every frame goes up the socket. */
  live: boolean;
  level: number;
  error: string | null;
  key: () => void;
  unkey: () => void;
}

/** Push to talk opens on the key; the other modes stay open and leave the gate to the server. */
export function captures(options: MicrophoneOptions, keyed: boolean): boolean {
  return options.heard && (keyed || options.send !== "push_to_talk");
}

/** Opens the microphone only while something hears it, and sends each frame up the socket. */
export function useMicrophone(
  node: string,
  socket: SdrSocket,
  options: MicrophoneOptions,
): Microphone {
  const [pressed, setPressed] = useState(false);
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const keyed = pressed || options.held;
  const capturing = captures(options, keyed);

  useEffect(() => {
    if (!capturing) {
      return;
    }
    let stopped = false;
    let capture: Capture | null = null;
    startCapture((frame, peak) => {
      socket.send({ type: "Speak", data: { node, pcm: encodeMicrophonePcm(frame) } });
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
    live: capturing && error === null,
    level,
    error,
    key,
    unkey,
  };
}
