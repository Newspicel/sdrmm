import { describe, expect, it } from "vitest";
import { pin, placeSlot } from "./graph";
import { applyGesture, moveGhost } from "./rackGesture";

const RACK = placeSlot(pin(pin({ slots: [] }, "a"), "b"), "b", { x: 6, y: 0, w: 6, h: 8 });

describe("rack gestures", () => {
  it("aims a move at the pointer and names the face it trades with", () => {
    const gesture = { node: "a", mode: "move" as const, base: RACK };
    const next = applyGesture(gesture, 6, 0);
    expect(moveGhost(gesture, 6, 0, next)).toEqual({
      cell: { x: 6, y: 0, w: 6, h: 4 },
      legal: true,
      partner: "b",
    });
    expect(moveGhost(gesture, 0, 4, applyGesture(gesture, 0, 4))?.partner).toBeNull();
  });

  it("marks a move onto two faces as blocked", () => {
    const three = placeSlot(pin(RACK, "c"), "c", { x: 0, y: 4, w: 6, h: 4 });
    const shrunk = placeSlot(three, "b", { x: 6, y: 0, w: 6, h: 4 });
    const crowded = placeSlot(pin(shrunk, "d"), "d", { x: 6, y: 4, w: 6, h: 4 });
    const gesture = { node: "a", mode: "move" as const, base: crowded };
    const next = applyGesture(gesture, 3, 2);
    expect(next).toBe(crowded);
    expect(moveGhost(gesture, 3, 2, next)).toEqual({
      cell: { x: 3, y: 2, w: 6, h: 4 },
      legal: false,
      partner: null,
    });
  });

  it("keeps resizing up to a blocker instead of freezing", () => {
    const gesture = { node: "a", mode: "corner" as const, base: RACK };
    expect(applyGesture(gesture, 4, 3).slots?.[0]).toEqual({ node: "a", x: 0, y: 0, w: 6, h: 7 });
    expect(moveGhost(gesture, 4, 3, applyGesture(gesture, 4, 3))).toBeNull();
  });
});
