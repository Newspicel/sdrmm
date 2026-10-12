import { useCallback, useEffect, useRef, useState } from "react";
import { runTool } from "../../lib/api";
import type { NanoVnaSweep, NanoVnaSweepRequest } from "../../lib/types";
import { nanoVnaSweep, nanoVnaSweepRequest } from "./nanovna";

export interface Sweeper {
  sweep: NanoVnaSweep | null;
  at: number;
  busy: boolean;
  live: boolean;
  error: string | null;
  count: number;
  once: (request: NanoVnaSweepRequest) => void;
  setLive: (live: boolean) => void;
}

export function useSweeper(request: NanoVnaSweepRequest | null): Sweeper {
  const [sweep, setSweep] = useState<{ sweep: NanoVnaSweep; at: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [live, setLiveState] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [count, setCount] = useState(0);
  const mounted = useRef(true);
  const latest = useRef(request);

  useEffect(() => {
    latest.current = request;
  }, [request]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const once = useCallback(async (next: NanoVnaSweepRequest) => {
    setBusy(true);
    try {
      const result = nanoVnaSweep(await runTool(nanoVnaSweepRequest(next)));
      if (!mounted.current) {
        return;
      }
      if (result === null) {
        throw new Error("The instrument answered without a sweep");
      }
      setSweep({ sweep: result, at: Date.now() });
      setError(null);
      setCount((value) => value + 1);
    } catch (failure) {
      if (mounted.current) {
        setError(failure instanceof Error ? failure.message : String(failure));
        setLiveState(false);
      }
    } finally {
      if (mounted.current) {
        setBusy(false);
      }
    }
  }, []);

  useEffect(() => {
    if (live && !busy && latest.current !== null) {
      void once(latest.current);
    }
  }, [live, busy, once]);

  const setLive = useCallback((next: boolean) => {
    setLiveState(next && latest.current !== null);
  }, []);

  return {
    sweep: sweep?.sweep ?? null,
    at: sweep?.at ?? 0,
    busy,
    live,
    error,
    count,
    once: (next) => void once(next),
    setLive,
  };
}
