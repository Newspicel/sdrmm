import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useMemo,
  useSyncExternalStore,
} from "react";

/** Checks a stored value still fits, so stale storage falls back instead of breaking a face. */
export type Valid<T> = (value: unknown) => value is T;

/** Choices kept here when the browser refuses storage, so they still hold for this page. */
const unstored = new Map<string, string>();
const listeners = new Map<string, Set<() => void>>();

/** One storage key per choice, scoped by workspace id and node id where they apply. */
export function rememberedKey(name: string, ...scope: readonly (string | number)[]): string {
  return [`sdrmm.${name}`, ...scope].join(":");
}

export function parseRemembered<T>(raw: string | null, fallback: T, valid: Valid<T>): T {
  if (raw === null) {
    return fallback;
  }
  try {
    const value: unknown = JSON.parse(raw);
    return valid(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

export function readRemembered<T>(key: string, fallback: T, valid: Valid<T>): T {
  return parseRemembered(stored(key), fallback, valid);
}

export function storeRemembered(key: string, value: unknown): void {
  const raw = JSON.stringify(value);
  try {
    localStorage.setItem(key, raw);
    unstored.delete(key);
  } catch {
    unstored.set(key, raw);
  }
  for (const listener of listeners.get(key) ?? []) {
    listener();
  }
}

/** Like useState, but kept in this browser and shared by every user of the same key. */
export function useRemembered<T>(
  key: string,
  fallback: T,
  valid: Valid<T>,
): [T, Dispatch<SetStateAction<T>>] {
  const subscribe = useCallback((listener: () => void) => listen(key, listener), [key]);
  const read = (): string | null => stored(key);
  const raw = useSyncExternalStore(subscribe, read, read);
  const value = useMemo(() => parseRemembered(raw, fallback, valid), [raw, fallback, valid]);
  const set = useCallback(
    (next: SetStateAction<T>) => {
      const current = readRemembered(key, fallback, valid);
      storeRemembered(key, typeof next === "function" ? (next as (held: T) => T)(current) : next);
    },
    [key, fallback, valid],
  );
  return [value, set];
}

export const isFiniteNumber: Valid<number> = (value): value is number =>
  typeof value === "number" && Number.isFinite(value);

export function oneOf<T>(choices: readonly T[]): Valid<T> {
  return (value): value is T => choices.some((choice) => choice === value);
}

export function within(min: number, max: number): Valid<number> {
  return (value): value is number => isFiniteNumber(value) && value >= min && value <= max;
}

export function listOf<T>(valid: Valid<T>): Valid<readonly T[]> {
  return (value): value is readonly T[] => Array.isArray(value) && value.every(valid);
}

export function orNull<T>(valid: Valid<T>): Valid<T | null> {
  return (value): value is T | null => value === null || valid(value);
}

export const isIndex: Valid<number> = (value): value is number =>
  isFiniteNumber(value) && Number.isInteger(value) && value >= 0;

export const isBoolean: Valid<boolean> = (value): value is boolean => typeof value === "boolean";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stored(key: string): string | null {
  const held = unstored.get(key);
  if (held !== undefined) {
    return held;
  }
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function listen(key: string, listener: () => void): () => void {
  const held = listeners.get(key) ?? new Set();
  listeners.set(key, held);
  held.add(listener);
  return () => {
    held.delete(listener);
    if (held.size === 0 && listeners.get(key) === held) {
      listeners.delete(key);
    }
  };
}
