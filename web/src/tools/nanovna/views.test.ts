import { describe, expect, it } from "vitest";
import { readouts } from "./analysis";
import { markerRows, placeMarker, stepMarker } from "./markers";
import { dropReference, MAX_REFERENCES, toggleReference } from "./references";
import { point, resonantSweep } from "./testdata";
import {
  LIVE_COLOR,
  nearestIndex,
  REFERENCE_COLORS,
  type Subject,
  swrDomain,
  tracesFor,
  viewFor,
  visibleValues,
} from "./views";

const ROWS = readouts(resonantSweep(101).points);

function subject(key: string, color: string): Subject {
  return { key, label: key, color, dashed: key !== "live", rows: ROWS };
}

describe("SWR scale", () => {
  it("shows a tuned antenna between 1 and a small whole number", () => {
    expect(swrDomain([1.1, 1.8, 2.6])).toEqual({ low: 1, high: 3 });
    expect(swrDomain([1.1, 7.2])).toEqual({ low: 1, high: 8 });
  });

  it("caps a mostly matched sweep at 10 so the dip stays readable", () => {
    expect(swrDomain([1.2, 400])).toEqual({ low: 1, high: 10 });
  });

  it("grows to the next decade when nothing comes near a match", () => {
    expect(swrDomain([180, 250, Number.POSITIVE_INFINITY])).toEqual({ low: 1, high: 1000 });
    expect(swrDomain([12, 40])).toEqual({ low: 1, high: 100 });
  });
});

describe("traces", () => {
  it("draws one line per subject for a single quantity, in that subject's colour", () => {
    const traces = tracesFor(viewFor("swr"), [
      subject("live", LIVE_COLOR),
      subject("stored:1", REFERENCE_COLORS[0]),
    ]);
    expect(traces.map((trace) => trace.color)).toEqual([LIVE_COLOR, REFERENCE_COLORS[0]]);
    expect(traces[1]?.dashed).toBe(true);
    expect(traces[0]?.values).toHaveLength(ROWS.length);
  });

  it("draws resistance and reactance of the main subject only", () => {
    const traces = tracesFor(viewFor("impedance"), [
      subject("live", LIVE_COLOR),
      subject("stored:1", REFERENCE_COLORS[0]),
    ]);
    expect(traces.map((trace) => trace.label)).toEqual(["R", "X"]);
  });

  it("scales to the values inside the zoom window", () => {
    const traces = tracesFor(viewFor("swr"), [subject("live", LIVE_COLOR)]);
    const centre = ROWS[50]?.frequencyHz ?? 0;
    const inside = visibleValues(traces, { low: centre - 1, high: centre + 1 });
    expect(inside).toHaveLength(1);
  });

  it("finds the nearest sample on either side", () => {
    expect(nearestIndex([10, 20, 30], 24)).toBe(1);
    expect(nearestIndex([10, 20, 30], 26)).toBe(2);
    expect(nearestIndex([10, 20, 30], -5)).toBe(0);
    expect(nearestIndex([10, 20, 30], 99)).toBe(2);
  });
});

describe("markers", () => {
  it("adds a new marker when the best match is active, then moves it", () => {
    const first = placeMarker([], "best", 14_000_000);
    expect(first).toEqual({ markers: [{ id: 1, hz: 14_000_000 }], active: 1 });
    const moved = placeMarker(first.markers, 1, 14_200_000);
    expect(moved.markers).toEqual([{ id: 1, hz: 14_200_000 }]);
  });

  it("reuses the last marker once four are placed", () => {
    let state = { markers: [] as { id: number; hz: number }[], active: 0 };
    for (const hz of [1, 2, 3, 4, 5]) {
      state = placeMarker(state.markers, "best", hz);
    }
    expect(state.markers.map((marker) => marker.hz)).toEqual([1, 2, 3, 5]);
  });

  it("steps along the sweep and stops at its ends", () => {
    const markers = [{ id: 1, hz: ROWS[0]?.frequencyHz ?? 0 }];
    expect(stepMarker(markers, 1, ROWS, -1)[0]?.hz).toBe(ROWS[0]?.frequencyHz);
    expect(stepMarker(markers, 1, ROWS, 10)[0]?.hz).toBe(ROWS[10]?.frequencyHz);
  });

  it("lists the best match first and snaps markers to measured points", () => {
    const rows = readouts([point(1_000, { re: 0, im: 0 }), point(2_000, { re: 0.5, im: 0 })]);
    const listed = markerRows([{ id: 1, hz: 1_900 }], rows[0] ?? null, rows);
    expect(listed.map((entry) => entry.id)).toEqual(["best", 1]);
    expect(listed[1]?.row.frequencyHz).toBe(2_000);
  });
});

function source(key: string) {
  return { key, label: key, points: [] };
}

describe("overlays", () => {
  it("keeps an overlay's colour when another one is hidden", () => {
    let references = toggleReference([], source("a"));
    references = toggleReference(references, source("b"));
    references = toggleReference(references, source("a"));
    references = toggleReference(references, source("c"));
    expect(references.map((reference) => [reference.key, reference.color])).toEqual([
      ["b", REFERENCE_COLORS[1]],
      ["c", REFERENCE_COLORS[0]],
    ]);
  });

  it("refuses a fifth overlay instead of inventing a colour", () => {
    let references = toggleReference([], source("a"));
    for (const key of ["b", "c", "d", "e"]) {
      references = toggleReference(references, source(key));
    }
    expect(references).toHaveLength(MAX_REFERENCES);
    expect(dropReference(references, "a")).toHaveLength(MAX_REFERENCES - 1);
  });
});
