import { type RefObject, useLayoutEffect, useRef } from "react";

const FADE_IN: Keyframe[] = [{ opacity: 0 }, { opacity: 1 }];

const GLIDE: KeyframeAnimationOptions = {
  duration: 160,
  easing: "cubic-bezier(0.22, 1, 0.36, 1)",
};

function prefersStill(): boolean {
  return (
    typeof matchMedia === "undefined" || matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export function useFadeOnChange<T extends HTMLElement>(value: unknown): RefObject<T | null> {
  const element = useRef<T>(null);
  const last = useRef(value);
  useLayoutEffect(() => {
    if (Object.is(last.current, value)) {
      return;
    }
    last.current = value;
    if (!prefersStill()) {
      element.current?.animate(FADE_IN, GLIDE);
    }
  }, [value]);
  return element;
}
