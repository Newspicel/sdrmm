import { describe, expect, it } from "vitest";
import { ellipsePath, ellipsePoint, senseArrow, senseStep } from "./polarimeter";

function points(path: string): [number, number][] {
  return [...path.matchAll(/[ML](-?[\d.]+) (-?[\d.]+)/g)].map((match) => [
    Number(match[1]),
    Number(match[2]),
  ]);
}

describe("ellipsePath", () => {
  it("closes the path with one point per step", () => {
    const path = ellipsePath(30, 10, 40, 48);
    expect(path.endsWith("Z")).toBe(true);
    expect(points(path)).toHaveLength(64);
  });

  it("draws a circle at 45 degrees of ellipticity", () => {
    const radius = 40 * Math.cos(Math.PI / 4);
    for (const [x, y] of points(ellipsePath(17, 45, 40, 48))) {
      expect(Math.hypot(x - 48, y - 48)).toBeCloseTo(radius, 1);
    }
  });

  it("draws a flat line tilted by the angle for linear polarisation", () => {
    const start = ellipsePoint(0, 90, 0, 40, 48);
    expect(start.x).toBeCloseTo(48);
    expect(start.y).toBeCloseTo(8);
    const flat = ellipsePoint(Math.PI / 2, 0, 0, 40, 48);
    expect(flat.x).toBeCloseTo(48);
    expect(flat.y).toBeCloseTo(48);
  });
});

function turn(angle: number, chi: number, v: number): number {
  const start = ellipsePoint(0, angle, chi, 40, 48);
  const next = ellipsePoint(senseStep(v, chi), angle, chi, 40, 48);
  return (start.x - 48) * (next.y - start.y) - (start.y - 48) * (next.x - start.x);
}

describe("rotation sense", () => {
  it("turns clockwise on screen for positive V and back for negative V", () => {
    for (const angle of [0, 35, 120]) {
      expect(turn(angle, 20, 0.4)).toBeGreaterThan(0);
      expect(turn(angle, -20, -0.4)).toBeLessThan(0);
      expect(turn(angle, -20, 0.4)).toBeGreaterThan(0);
      expect(turn(angle, 20, -0.4)).toBeLessThan(0);
    }
  });

  it("draws an arrowhead only for elliptical waves", () => {
    expect(senseArrow(30, 1, 0.1, 40, 48)).toBeNull();
    const arrow = senseArrow(30, 20, 0.4, 40, 48);
    expect(arrow?.head.endsWith("Z")).toBe(true);
    expect(points(arrow?.head ?? "")).toHaveLength(3);
  });
});
