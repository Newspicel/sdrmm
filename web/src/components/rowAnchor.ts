import { type RefObject, useLayoutEffect, useRef } from "react";

export interface Anchor {
  key: string;
  top: number;
}

export function anchorShift(previous: Anchor | null, next: Anchor | null): number {
  if (previous === null || next === null || previous.key !== next.key) {
    return 0;
  }
  return next.top - previous.top;
}

export function useRowAnchor(
  scroller: RefObject<HTMLElement | null>,
  key: string | null,
): RefObject<HTMLTableRowElement | null> {
  const row = useRef<HTMLTableRowElement | null>(null);
  const anchor = useRef<Anchor | null>(null);
  useLayoutEffect(() => {
    const element = row.current;
    const next = key === null || element === null ? null : { key, top: element.offsetTop };
    const shift = anchorShift(anchor.current, next);
    if (shift !== 0) {
      scroller.current?.scrollBy({ top: shift });
    }
    anchor.current = next;
  });
  return row;
}
