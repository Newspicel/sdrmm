import { describe, expect, it } from "vitest";
import {
  blankEntry,
  CURRENT,
  choiceItems,
  chosenValue,
  committed,
  conditionHolds,
  effectiveControl,
  entryComplete,
  type Form,
  fieldValue,
  isQuiet,
  parseHz,
  presetIndex,
  shownFields,
  valueAt,
  withPreset,
  withValueAt,
} from "./settingsForm";
import type { Control, SettingField } from "./types";

type Choice = Extract<Control, { type: "choice" }>;

function field(
  key: string,
  control: SettingField["control"],
  over: Partial<SettingField> = {},
): SettingField {
  return { key, label: key, control, ...over };
}

const TOGGLE = { type: "toggle" } as const;
const NUMBER = { type: "number", min: 0, max: 100 } as const;

const MODES: Choice = {
  type: "choice",
  options: [
    { value: "off", label: "Off" },
    { value: "tone", label: "Tone" },
  ],
};

const TONE: Form = [
  field("mode", MODES, { fallback: "off" }),
  field("tone_hz", NUMBER, {
    when: [{ key: "mode", any_of: ["tone"] }],
    fallback: 88.5,
  }),
];

describe("dotted keys", () => {
  it("reads and writes nested objects", () => {
    const settings = { header: { length: 12 } };
    expect(valueAt(settings, "header.length")).toBe(12);
    expect(valueAt(settings, "header.crc")).toBeUndefined();
    expect(withValueAt(settings, "header.crc", true)).toEqual({
      header: { length: 12, crc: true },
    });
    expect(withValueAt({}, "header.length", 8)).toEqual({ header: { length: 8 } });
  });

  it("clears an optional nested number to null for the server to drop", () => {
    expect(withValueAt({ header: { length: 12 } }, "header.length", null)).toEqual({
      header: { length: null },
    });
  });
});

describe("when", () => {
  it("shows a field only while every condition holds", () => {
    const form: Form = [
      field("a", TOGGLE),
      field("b", TOGGLE),
      field("both", TOGGLE, {
        when: [
          { key: "a", any_of: [true] },
          { key: "b", any_of: [true] },
        ],
      }),
    ];
    expect(shownFields(form, { a: true, b: false }).map((f) => f.key)).toEqual(["a", "b"]);
    expect(shownFields(form, { a: true, b: true }).map((f) => f.key)).toContain("both");
  });

  it("holds an empty any_of while the key is set", () => {
    const condition = { key: "service", any_of: [] };
    expect(conditionHolds([], { service: 3 }, condition)).toBe(true);
    expect(conditionHolds([], { service: null }, condition)).toBe(false);
    expect(conditionHolds([], {}, condition)).toBe(false);
  });

  it("reads the controlling field's fallback while it is unset", () => {
    const form: Form = [TONE[0] as SettingField, { ...(TONE[1] as SettingField), fallback: null }];
    expect(shownFields(form, {}).map((f) => f.key)).toEqual(["mode"]);
    const tone: Form = [
      { ...(TONE[0] as SettingField), fallback: "tone" },
      TONE[1] as SettingField,
    ];
    expect(shownFields(tone, {}).map((f) => f.key)).toEqual(["mode", "tone_hz"]);
  });
});

describe("fallback", () => {
  it("shows the fallback while a setting is unset", () => {
    expect(fieldValue(TONE[1] as SettingField, {})).toBe(88.5);
    expect(fieldValue(TONE[1] as SettingField, { tone_hz: 100 })).toBe(100);
  });

  it("sets a revealed field to its fallback when the controlling field switches to it", () => {
    expect(committed(TONE, {}, "mode", "tone")).toEqual({ mode: "tone", tone_hz: 88.5 });
    expect(committed(TONE, { tone_hz: 123 }, "mode", "tone")).toEqual({
      mode: "tone",
      tone_hz: 123,
    });
    expect(committed(TONE, { mode: "tone", tone_hz: 90 }, "tone_hz", 91)).toEqual({
      mode: "tone",
      tone_hz: 91,
    });
  });
});

describe("quiet", () => {
  it("compares JSON values, arrays included", () => {
    const service = field("service", NUMBER, { quiet: [null] });
    expect(isQuiet(service, null)).toBe(true);
    expect(isQuiet(service, 3)).toBe(false);
    const keys = field("keys", { type: "list", item: [] }, { quiet: [[] as never] });
    expect(isQuiet(keys, [])).toBe(true);
    expect(isQuiet(keys, null)).toBe(true);
    expect(isQuiet(keys, [{ name: "x" }])).toBe(false);
    const pair = field("pair", TOGGLE, { quiet: [[1, 2] as never] });
    expect(isQuiet(pair, [1, 2])).toBe(true);
    expect(isQuiet(pair, [2, 1])).toBe(false);
  });
});

describe("live", () => {
  const service = field("service", { type: "live", fallback: NUMBER });

  it("stands in with its fallback control until the status publishes options", () => {
    expect(effectiveControl(service, [])).toEqual(NUMBER);
    expect(effectiveControl(service, [{ name: "service", options: [] }])).toEqual(NUMBER);
  });

  it("offers the options the status publishes under its key", () => {
    const options = [{ value: 1, label: "Radio One" }];
    expect(
      effectiveControl(service, [
        { name: "other", options: [{ value: 9, label: "Nine" }] },
        { name: "service", options },
      ]),
    ).toEqual({ type: "choice", options });
  });
});

describe("choice", () => {
  it("keeps a current value that is not among the options as its own option", () => {
    const items = choiceItems(MODES.options, "squelch");
    expect(items.chosen).toBe(CURRENT);
    expect(items.options.at(-1)).toEqual({ value: CURRENT, label: "squelch" });
    expect(chosenValue(MODES.options, CURRENT, "squelch")).toBe("squelch");
  });

  it("matches options by JSON value", () => {
    const options = [
      { value: null, label: "Auto" },
      { value: { band: 2 }, label: "Two" },
    ];
    expect(choiceItems(options, null).chosen).toBe("0");
    expect(choiceItems(options, { band: 2 }).chosen).toBe("1");
    expect(chosenValue(options, "1", null)).toEqual({ band: 2 });
  });
});

describe("preset", () => {
  const options = [
    { label: "Slow", values: { width: 125 as never, rate: 12 as never } },
    { label: "Fast", values: { width: 500 as never, rate: 7 as never } },
  ];
  const form: Form = [field("width", NUMBER, { fallback: 125 }), field("rate", NUMBER)];

  it("picks the option whose values all match, the fallback counting", () => {
    expect(presetIndex(form, { rate: 12 }, options)).toBe(0);
    expect(presetIndex(form, { width: 500, rate: 7 }, options)).toBe(1);
    expect(presetIndex(form, { width: 500, rate: 12 }, options)).toBe(-1);
  });

  it("writes every value of the chosen option", () => {
    expect(withPreset(form, { width: 125, rate: 12, other: 1 }, options[1] as never)).toEqual({
      width: 500,
      rate: 7,
      other: 1,
    });
  });
});

describe("list entries", () => {
  const KIND: Choice = {
    type: "choice",
    options: [
      { value: "channel", label: "Channel" },
      { value: "session", label: "Session" },
    ],
  };
  const item: Form = [
    field("kind", KIND, { fallback: "channel" }),
    field("name", { type: "text" }, { when: [{ key: "kind", any_of: ["channel"] }] }),
    field(
      "phase",
      { type: "text", optional: true },
      { when: [{ key: "kind", any_of: ["channel"] }] },
    ),
    field(
      "name",
      { type: "text" },
      { label: "Address", when: [{ key: "kind", any_of: ["session"] }] },
    ),
    field(
      "key",
      { type: "text", secret: true },
      { when: [{ key: "kind", any_of: ["session"] }] },
    ),
  ];

  it("start from the fallbacks they show", () => {
    expect(blankEntry(item)).toEqual({ kind: "channel" });
  });

  it("refer their conditions to the entry, two fields sharing a key", () => {
    expect(shownFields(item, { kind: "channel" }).map((f) => f.label)).toEqual([
      "kind",
      "name",
      "phase",
    ]);
    expect(shownFields(item, { kind: "session" }).map((f) => f.label)).toEqual([
      "kind",
      "Address",
      "key",
    ]);
  });

  it("complete once every visible field that is not optional is filled", () => {
    expect(entryComplete(item, { kind: "channel" })).toBe(false);
    expect(entryComplete(item, { kind: "channel", name: "LongFast" })).toBe(true);
    expect(entryComplete(item, { kind: "session", name: "26011234" })).toBe(false);
    expect(entryComplete(item, { kind: "session", name: "26011234", key: "00ff" })).toBe(true);
  });
});

describe("parseHz", () => {
  it("reads SI prefixes, a bare number being hertz", () => {
    expect(parseHz("12500")).toBe(12_500);
    expect(parseHz("2.4 kHz")).toBe(2_400);
    expect(parseHz("145.5 MHz")).toBe(145_500_000);
    expect(parseHz("1,2g")).toBe(1_200_000_000);
    expect(parseHz("-600 Hz")).toBe(-600);
  });

  it("refuses anything else", () => {
    expect(parseHz("")).toBeNull();
    expect(parseHz("fast")).toBeNull();
    expect(parseHz("12 dB")).toBeNull();
  });
});
