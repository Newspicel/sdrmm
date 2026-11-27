import { useSyncExternalStore } from "react";

const COARSE = "(pointer: coarse)";

const listeners = new Set<() => void>();
let touch = false;

/// Touch laptops report a fine primary pointer, so the pointer last pressed decides, not `pointer: coarse`.
export function initTouch(target: Window): void {
  set(target.document, target.matchMedia(COARSE).matches);
  target.addEventListener(
    "pointerdown",
    (event) => set(target.document, touchesWith(event.pointerType, touch)),
    { capture: true, passive: true },
  );
}

export function touchesWith(pointerType: string, current: boolean): boolean {
  if (pointerType === "touch") {
    return true;
  }
  return pointerType === "mouse" ? false : current;
}

export function useTouch(): boolean {
  return useSyncExternalStore(subscribe, snapshot, serverSnapshot);
}

function set(document: Document, next: boolean): void {
  if (next === touch && document.documentElement.hasAttribute("data-touch") === next) {
    return;
  }
  touch = next;
  document.documentElement.toggleAttribute("data-touch", next);
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function snapshot(): boolean {
  return touch;
}

function serverSnapshot(): boolean {
  return false;
}
