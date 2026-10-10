import { type Dispatch, type SetStateAction, useCallback, useState } from "react";

interface Scoped<T> {
  scope: number | null;
  value: T;
}

export function scopedValue<T>(held: Scoped<T>, scope: number | null, initial: T): T {
  return held.scope === scope ? held.value : initial;
}

export function useScopedState<T>(
  scope: number | null,
  initial: T,
): [T, Dispatch<SetStateAction<T>>] {
  const [held, setHeld] = useState<Scoped<T>>({ scope, value: initial });
  const set = useCallback(
    (next: SetStateAction<T>) =>
      setHeld((previous) => {
        const base = scopedValue(previous, scope, initial);
        const value = typeof next === "function" ? (next as (previous: T) => T)(base) : next;
        return { scope, value };
      }),
    [scope, initial],
  );
  return [scopedValue(held, scope, initial), set];
}
