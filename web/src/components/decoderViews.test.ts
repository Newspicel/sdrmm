import { describe, expect, it } from "vitest";
import type { Field } from "../lib/types";
import {
  ageClass,
  appendTranscript,
  buildTranscript,
  fieldColumns,
  fieldKeyed,
  formatAge,
  formatClock,
  isAtBottom,
  namedColumns,
  TARGET_MAX_AGE_MS,
  TARGET_STALE_MS,
} from "./decoderViews";

function text(label: string, value: string, key = label.toLowerCase()): Field {
  return { key, label, value: { type: "text", value } };
}

describe("ageing", () => {
  it("dims a target before it disappears", () => {
    expect(ageClass(0)).toBe("text-ink");
    expect(ageClass(TARGET_STALE_MS - 1)).toBe("text-ink");
    expect(ageClass(TARGET_STALE_MS)).toBe("text-ink-dim");
    expect(ageClass(TARGET_MAX_AGE_MS / 2)).toBe("text-ink-dim opacity-45");
  });

  it("formats age as seconds, m:ss then hours", () => {
    expect(formatAge(0)).toBe("0s");
    expect(formatAge(59_400)).toBe("59s");
    expect(formatAge(60_000)).toBe("1:00");
    expect(formatAge(3_599_000)).toBe("59:59");
    expect(formatAge(3_600_000)).toBe("1h00");
    expect(formatAge(-5_000)).toBe("0s");
  });

  it("renders a clock, and a placeholder for an unparsable stamp", () => {
    const local = new Date(2026, 7, 9, 12, 34, 56).toISOString();
    expect(formatClock(local)).toBe("12:34:56");
    expect(formatClock("not a date")).toBe("--:--:--");
  });
});

describe("transcripts", () => {
  it("appends until the limit, then drops the head at a line boundary", () => {
    expect(appendTranscript("ab", "cd", 10)).toBe("abcd");
    expect(appendTranscript("abcdef", "gh", 4)).toBe("efgh");
    expect(appendTranscript("one\ntwo\nthree\n", "four\n", 12)).toBe("three\nfour\n");
  });

  it("builds the pane oldest-first from newest-first records", () => {
    const records = [
      { view: { summary: "", text: "CQ " } },
      { view: { summary: "", text: "TEST " } },
    ];
    expect(buildTranscript(records)).toBe("TEST CQ ");
    expect(buildTranscript([{ view: null }])).toBe("");
  });

  it("treats a near-bottom scroll as bottom", () => {
    expect(isAtBottom({ scrollTop: 900, scrollHeight: 1000, clientHeight: 100 })).toBe(true);
    expect(isAtBottom({ scrollTop: 895, scrollHeight: 1000, clientHeight: 100 })).toBe(true);
    expect(isAtBottom({ scrollTop: 500, scrollHeight: 1000, clientHeight: 100 })).toBe(false);
  });
});

describe("field columns", () => {
  it("joins the keys of every entity in order of first appearance", () => {
    const entities = [
      { view: { summary: "", fields: [text("Alt", "1"), text("Speed", "2")] } },
      { view: { summary: "", fields: [text("Squawk", "7000"), text("Höhe", "3", "alt")] } },
      { view: { summary: "" } },
    ];
    expect(fieldColumns(entities)).toEqual([
      { key: "alt", label: "Alt" },
      { key: "speed", label: "Speed" },
      { key: "squawk", label: "Squawk" },
    ]);
  });

  it("heads named columns by label and keeps an unseen key", () => {
    expect(namedColumns(["speed", "gone"], [[text("Speed", "2")]])).toEqual([
      { key: "speed", label: "Speed" },
      { key: "gone", label: "gone" },
    ]);
  });

  it("finds a field by its key, never its label", () => {
    const alt = text("Altitude", "1", "alt");
    expect(fieldKeyed([alt], "alt")?.value).toEqual({ type: "text", value: "1" });
    expect(fieldKeyed([alt], "Altitude")).toBeUndefined();
    expect(fieldKeyed(undefined, "alt")).toBeUndefined();
  });
});
