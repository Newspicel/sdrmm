import { type RefObject, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { type Anchors, anchorsOf, sameAnchors, visualCenter } from "./portAnchor";

export interface AnchorSlot {
  track: (port: string, mark: HTMLElement) => () => void;
}

export function useFaceLayout(): {
  container: RefObject<HTMLDivElement | null>;
  anchors: Anchors;
  slot: AnchorSlot;
} {
  const container = useRef<HTMLDivElement>(null);
  const marks = useRef(new Map<string, HTMLElement>());
  const watch = useRef<ResizeObserver | null>(null);
  const [anchors, setAnchors] = useState<Anchors>({});

  const measure = useCallback(() => {
    const host = container.current;
    if (host === null) {
      return;
    }
    const next = anchorsOf(marks.current, (mark: HTMLElement) => centreOn(mark, host));
    setAnchors((current) => (sameAnchors(current, next) ? current : next));
  }, []);

  const track = useCallback(
    (port: string, mark: HTMLElement) => {
      marks.current.set(port, mark);
      watch.current?.observe(rowOf(mark));
      measure();
      return () => {
        if (marks.current.get(port) === mark) {
          marks.current.delete(port);
        }
        watch.current?.unobserve(rowOf(mark));
        measure();
      };
    },
    [measure],
  );

  useLayoutEffect(measure);
  useLayoutEffect(() => watchMoves(container.current, marks.current, watch, measure), [measure]);

  const slot = useMemo(() => ({ track }), [track]);
  return { container, anchors, slot };
}

function centreOn(mark: Element, host: HTMLElement): number {
  const box = host.getBoundingClientRect();
  const scale = host.offsetHeight === 0 ? 1 : box.height / host.offsetHeight;
  return visualCenter(mark.getBoundingClientRect(), box, scale, host.clientTop);
}

function watchMoves(
  host: HTMLDivElement | null,
  marks: ReadonlyMap<string, HTMLElement>,
  watch: RefObject<ResizeObserver | null>,
  measure: () => void,
): (() => void) | undefined {
  if (host === null) {
    return undefined;
  }
  const remeasure = () => flushSync(measure);
  host.addEventListener("scroll", remeasure, { capture: true, passive: true });
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(remeasure);
  observer?.observe(host);
  for (const mark of marks.values()) {
    observer?.observe(rowOf(mark));
  }
  watch.current = observer;
  return () => {
    host.removeEventListener("scroll", remeasure, { capture: true });
    observer?.disconnect();
    watch.current = null;
  };
}

function rowOf(mark: HTMLElement): Element {
  return mark.parentElement ?? mark;
}

export function useContentHeight(inner: RefObject<HTMLElement | null>): number | null {
  const [height, setHeight] = useState<number | null>(null);
  useLayoutEffect(() => {
    const element = inner.current;
    if (element === null) {
      return undefined;
    }
    const read = () => setHeight(element.offsetHeight);
    read();
    if (typeof ResizeObserver === "undefined") {
      return undefined;
    }
    const observer = new ResizeObserver(read);
    observer.observe(element);
    return () => observer.disconnect();
  }, [inner]);
  return height;
}
