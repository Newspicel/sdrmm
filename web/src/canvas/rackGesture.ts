import type { RackLayout, RackSlot } from "../lib/types";
import {
  dragCorner,
  dragEdge,
  moveSlot,
  RACK_COLS,
  RACK_ROWS,
  type RackCell,
  type RackEdge,
} from "./graph";

export type GestureMode = "move" | "corner" | RackEdge;

export interface Gesture {
  node: string;
  mode: GestureMode;
  base: RackLayout;
}

export interface Ghost {
  cell: RackCell;
  legal: boolean;
  partner: string | null;
}

export function applyGesture(gesture: Gesture, dx: number, dy: number): RackLayout {
  const slot = slotOf(gesture.base, gesture.node);
  if (slot === undefined) {
    return gesture.base;
  }
  switch (gesture.mode) {
    case "move":
      return moveSlot(gesture.base, gesture.node, movedTo(slot, dx, dy));
    case "corner":
      return dragCorner(gesture.base, gesture.node, {
        w: clamp(slot.w + dx, 1, RACK_COLS - slot.x),
        h: clamp(slot.h + dy, 1, RACK_ROWS - slot.y),
      });
    default:
      return dragEdge(
        gesture.base,
        gesture.node,
        gesture.mode,
        gesture.mode === "n" || gesture.mode === "s" ? dy : dx,
      );
  }
}

/// Where a moved face is aimed, whether it may land there, and which face it trades places with.
export function moveGhost(
  gesture: Gesture,
  dx: number,
  dy: number,
  next: RackLayout,
): Ghost | null {
  const slot = slotOf(gesture.base, gesture.node);
  if (gesture.mode !== "move" || slot === undefined) {
    return null;
  }
  const traded = (next.slots ?? []).find(
    (candidate) =>
      candidate.node !== gesture.node && !sameCell(candidate, slotOf(gesture.base, candidate.node)),
  );
  return {
    cell: { ...movedTo(slot, dx, dy), w: slot.w, h: slot.h },
    legal: next !== gesture.base,
    partner: traded?.node ?? null,
  };
}

/// A press on a face's title bar, but not on its buttons or name field, picks the face up.
export function fromHeader(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest("header") !== null &&
    target.closest(".nodrag") === null
  );
}

function movedTo(slot: RackSlot, dx: number, dy: number): { x: number; y: number } {
  return {
    x: clamp(slot.x + dx, 0, RACK_COLS - slot.w),
    y: clamp(slot.y + dy, 0, RACK_ROWS - slot.h),
  };
}

function slotOf(rack: RackLayout, node: string): RackSlot | undefined {
  return (rack.slots ?? []).find((candidate) => candidate.node === node);
}

function sameCell(a: RackCell, b: RackCell | undefined): boolean {
  return b !== undefined && a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), Math.max(high, low));
}
