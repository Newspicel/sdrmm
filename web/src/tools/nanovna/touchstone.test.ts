import { describe, expect, it } from "vitest";
import type { NanoVnaComplex, NanoVnaPoint } from "../../lib/types";
import { sweepSource, TOUCHSTONE_FORMATS, touchstoneS1p, touchstoneS2p } from "./export";
import { point, resonantSweep, sweepOf } from "./testdata";
import { type Parsed, parseTouchstone, type TouchstoneFile, touchstonePorts } from "./touchstone";

const TWO_PORT = sweepOf(
  resonantSweep(41).points.map((entry, index) =>
    point(entry.frequency_hz, entry.s11, {
      re: 0.8 * Math.cos(index / 7),
      im: 0.8 * Math.sin(index / 7),
    }),
  ),
);

function parsed(result: Parsed<TouchstoneFile>): TouchstoneFile {
  if (!result.ok) {
    throw new Error(result.error);
  }
  return result.value;
}

function errorOf(result: Parsed<TouchstoneFile>): string {
  return result.ok ? "" : result.error;
}

function expectClose(actual: NanoVnaComplex | undefined, expected: NanoVnaComplex): void {
  expect(actual?.re).toBeCloseTo(expected.re, 5);
  expect(actual?.im).toBeCloseTo(expected.im, 5);
}

function expectSamePoints(actual: readonly NanoVnaPoint[], expected: readonly NanoVnaPoint[]) {
  expect(actual).toHaveLength(expected.length);
  expected.forEach((entry, index) => {
    expect(actual[index]?.frequency_hz).toBe(entry.frequency_hz);
    expectClose(actual[index]?.s11, entry.s11);
    expectClose(actual[index]?.s21, entry.s21);
  });
}

describe("touchstonePorts", () => {
  it("reads the port count from the extension", () => {
    expect(touchstonePorts("antenna.s1p")).toBe(1);
    expect(touchstonePorts("FILTER.S2P")).toBe(2);
    expect(touchstonePorts("sweep.csv")).toBeNull();
    expect(touchstonePorts("s1p")).toBeNull();
  });
});

describe("parseTouchstone", () => {
  it.each(TOUCHSTONE_FORMATS.map((entry) => entry.value))(
    "reads back a one-port %s export",
    (format) => {
      const sweep = resonantSweep(51);
      const file = parsed(parseTouchstone(touchstoneS1p(sweepSource(sweep), format), 1));
      expect(file.ports).toBe(1);
      expect(file.referenceOhms).toBe(50);
      expectSamePoints(file.points, sweep.points);
    },
  );

  it.each(TOUCHSTONE_FORMATS.map((entry) => entry.value))(
    "reads back a two-port %s export",
    (format) => {
      const file = parsed(parseTouchstone(touchstoneS2p(sweepSource(TWO_PORT), format), 2));
      expect(file.ports).toBe(2);
      expectSamePoints(file.points, TWO_PORT.points);
    },
  );

  it("keeps comment text without the marker", () => {
    const file = parsed(parseTouchstone(touchstoneS1p(sweepSource(resonantSweep(3)), "ri"), 1));
    expect(file.comments).toContain("freq S11");
    expect(file.comments.every((comment) => !comment.startsWith("!"))).toBe(true);
  });

  it("scales frequency units", () => {
    const text = ["# mhz s ri r 50", "14.1 0.5 0 ! first", "14.2 0.25 -0.25"].join("\n");
    const file = parsed(parseTouchstone(text, 1));
    expect(file.points.map((entry) => entry.frequency_hz)).toEqual([14_100_000, 14_200_000]);
    expect(file.points[0]?.s21).toEqual({ re: 0, im: 0 });
    expect(file.comments).toEqual(["first"]);
  });

  it("falls back to GHz, magnitude/angle and 50 ohm without an option line", () => {
    const file = parsed(parseTouchstone("1 0.5 90\n2 1 180\n", 1));
    expect(file.points[0]?.frequency_hz).toBe(1_000_000_000);
    expectClose(file.points[0]?.s11, { re: 0, im: 0.5 });
    expectClose(file.points[1]?.s11, { re: -1, im: 0 });
  });

  it("reads two-port rows that wrap across lines", () => {
    const text = [
      "# Hz S RI R 50",
      "1000000 0.1 0.2 0.3 0.4",
      "  0 0 0 0",
      "2000000 0.5 0.6",
      "  0.7 0.8 0 0 0 0",
    ].join("\n");
    const file = parsed(parseTouchstone(text, 2));
    expect(file.points).toEqual([
      point(1_000_000, { re: 0.1, im: 0.2 }, { re: 0.3, im: 0.4 }),
      point(2_000_000, { re: 0.5, im: 0.6 }, { re: 0.7, im: 0.8 }),
    ]);
  });

  it("renormalises a 75 ohm reflection to 50 ohm and leaves S21 alone", () => {
    const text = ["# Hz S RI R 75", "1000 0 0 0.5 0.1 0 0 0 0", "2000 1 0 0.5 0.1 0 0 0 0"].join(
      "\n",
    );
    const file = parsed(parseTouchstone(text, 2));
    expect(file.referenceOhms).toBe(75);
    expectClose(file.points[0]?.s11, { re: 0.2, im: 0 });
    expectClose(file.points[1]?.s11, { re: 1, im: 0 });
    expect(file.points[0]?.s21).toEqual({ re: 0.5, im: 0.1 });
  });

  it.each([
    ["", "Need at least 2 points"],
    ["# Hz S RI R 50\n1 0 0\n", "Need at least 2 points"],
    ["# Hz S RI R 50\n1 0 0\n2 0\n", "Incomplete data row"],
    ["# Hz S RI R 50\n2 0 0\n1 0 0\n", "Point 2: frequency not increasing"],
    ["# Hz S RI R 50\n1 0 0\n1 0 0\n", "Point 2: frequency not increasing"],
    ["# Hz S RI R 50\n1 0 x\n2 0 0\n", 'Line 2: "x" is not a number'],
    ["# Hz Y RI R 50\n1 0 0\n2 0 0\n", "Only S parameters are supported"],
    ["# Hz S RI R 0\n1 0 0\n2 0 0\n", "Bad reference impedance"],
    ["# Hz S XY R 50\n1 0 0\n2 0 0\n", 'Unknown option "xy"'],
    ["[Version] 2.0\n# Hz S RI R 50\n", "Touchstone 2 files are not supported"],
    ["# Hz S RI R 50\n1 0 0\n2 1e400 0\n", 'Line 3: "1e400" is not a number'],
    ["# Hz S RI R 75\n1 0 0\n2 -5 0\n", "Point 2: value out of range"],
  ])("rejects %j", (text, error) => {
    expect(errorOf(parseTouchstone(text, 1))).toBe(error);
  });
});
