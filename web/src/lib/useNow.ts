import { useEffect, useState } from "react";

/// The wall clock, ticking every `everyMs` while the tab is shown and once on its return.
export function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const tick = (): void => {
      if (!document.hidden) {
        setNow(Date.now());
      }
    };
    const timer = setInterval(tick, everyMs);
    document.addEventListener("visibilitychange", tick);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  }, [everyMs]);
  return now;
}
