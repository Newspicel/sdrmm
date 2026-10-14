import type { Key, ReactElement } from "react";

export interface Leaving {
  element: ReactElement;
  at: number;
}

export interface Held {
  last: readonly ReactElement[];
  leaving: ReadonlyMap<Key, Leaving>;
  entered: ReadonlySet<Key>;
}

export interface Shown {
  key: Key;
  element: ReactElement;
  leaving: boolean;
}

export function keyOf(element: ReactElement, index: number): Key {
  return element.key ?? String(index);
}

export function heldFirst(children: readonly ReactElement[]): Held {
  return { last: children, leaving: new Map(), entered: new Set() };
}

export function advance(held: Held, children: readonly ReactElement[]): Held {
  const now = new Set(children.map(keyOf));
  const before = new Set(held.last.map(keyOf));
  const leaving = new Map([...held.leaving].filter(([key]) => !now.has(key)));
  held.last.forEach((element, at) => {
    const key = keyOf(element, at);
    if (!now.has(key)) {
      leaving.set(key, { element, at });
    }
  });
  const entered = new Set([...held.entered, ...[...now].filter((key) => !before.has(key))]);
  return { last: children, leaving, entered };
}

export function gone(held: Held, key: Key): Held {
  const leaving = new Map(held.leaving);
  leaving.delete(key);
  return { ...held, leaving };
}

export function shown(children: readonly ReactElement[], held: Held): Shown[] {
  const rows: Shown[] = children.map((element, at) => ({
    key: keyOf(element, at),
    element,
    leaving: false,
  }));
  const going = [...held.leaving].toSorted(([, a], [, b]) => a.at - b.at);
  for (const [key, { element, at }] of going) {
    rows.splice(Math.min(at, rows.length), 0, { key, element, leaving: true });
  }
  return rows;
}
