import { describe, expect, it } from "vitest";
import {
  blankEntry,
  CURRENT,
  ceiling,
  choiceItems,
  chosenValue,
  committed,
  conditionHolds,
  effectiveControl,
  entryComplete,
  type Form,
  fieldValue,
  isQuiet,
  lanesOf,
  lanesValue,
  optionLabel,
  parseHz,
  presetIndex,
  sections,
  shownFields,
  shownNumber,
  shownSpec,
  storedNumber,
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

  it("reads and writes list entries by index", () => {
    const settings = { stations: [{ name: "a" }, { name: "b", hops: [1, 2] }] };
    expect(valueAt(settings, "stations.1.name")).toBe("b");
    expect(valueAt(settings, "stations.1.hops.0")).toBe(1);
    expect(valueAt(settings, "stations.x")).toBeUndefined();
    expect(withValueAt(settings, "stations.1.hops.1", 5)).toEqual({
      stations: [{ name: "a" }, { name: "b", hops: [1, 5] }],
    });
    expect(settings.stations[1]?.hops).toEqual([1, 2]);
  });

  it("applies a preset with dotted keys inside a list entry", () => {
    const item: Form = [field("timing.rate", NUMBER), field("mode", MODES)];
    const slow = { label: "Slow", values: { "timing.rate": 12, mode: "tone" } };
    const entry = withPreset(item, { name: "a" }, slow);
    expect(entry).toEqual({ name: "a", timing: { rate: 12 }, mode: "tone" });
    expect(presetIndex(item, entry, [slow])).toBe(0);
  });
});

function status(key: string): string | undefined {
  return key === "mode" ? "TM2" : undefined;
}

describe("option status", () => {
  const auto = { value: null, label: "Auto", status: "mode" };
  const options = [auto, { value: 1, label: "TM1" }];

  it("follows an option's label with the readout it names", () => {
    expect(optionLabel(auto, status)).toBe("Auto TM2");
    expect(choiceItems(options, null, status).options.map((item) => item.label)).toEqual([
      "Auto TM2",
      "TM1",
    ]);
  });

  it("keeps the bare label while no readout answers", () => {
    expect(choiceItems(options, null).options[0]?.label).toBe("Auto");
    expect(choiceItems(options, null, () => "").options[0]?.label).toBe("Auto");
  });
});

describe("number scale", () => {
  const spec = {
    unit: "MHz",
    scale: 1e6,
    min: 24e6,
    max: 1.7e9,
    step: 12_500,
    presets: [{ value: 145_500_000, label: "Calling" }],
  };

  it("shows stored values divided by the scale", () => {
    expect(shownNumber(spec, 145_500_000)).toBe(145.5);
    expect(shownNumber(spec, null)).toBeNull();
    expect(shownSpec(spec)).toEqual({
      ...spec,
      min: 24,
      max: 1700,
      step: 0.0125,
      presets: [{ value: 145.5, label: "Calling" }],
    });
  });

  it("stores shown values multiplied back, without float noise", () => {
    expect(storedNumber(spec, 144.8)).toBe(144_800_000);
    expect(storedNumber(spec, null)).toBeNull();
  });

  it("leaves an unscaled spec alone", () => {
    const plain = { min: 0, max: 10 };
    expect(shownSpec(plain)).toBe(plain);
    expect(shownNumber(plain, 7)).toBe(7);
    expect(storedNumber(plain, 7)).toBe(7);
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
    field("key", { type: "text", secret: true }, { when: [{ key: "kind", any_of: ["session"] }] }),
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

describe("linked fields", () => {
  const bins = field("bins", { type: "number" });
  const columns = field("columns", { type: "number", max: 1024 }, { at_most: "bins" });
  const main = field("main", { type: "lanes", pick: "one" });
  const refs = field("refs", { type: "lanes", pick: "list", except: "main" });
  const mask = field("mask", { type: "lanes", pick: "mask", except: "main" });

  it("pulls a field down with the setting it may not pass", () => {
    const form = [bins, columns];
    expect(committed(form, { bins: 1024, columns: 512 }, "bins", 256)).toEqual({
      bins: 256,
      columns: 256,
    });
    expect(committed(form, { bins: 256, columns: 128 }, "bins", 512).columns).toBe(128);
    expect(ceiling(columns, { bins: 256 }, 1024)).toBe(256);
    expect(ceiling(columns, {}, 1024)).toBe(1024);
  });

  it("drops a lane from the fields that may not hold it", () => {
    const form = [main, refs, mask];
    const next = committed(form, { main: 0, refs: [1, 2], mask: 0b110 }, "main", 2);
    expect(next).toEqual({ main: 2, refs: [1], mask: 0b010 });
  });

  it("reads and writes lanes in every encoding", () => {
    expect(lanesOf("mask", 0b1010)).toEqual([1, 3]);
    expect(lanesValue("mask", [3, 1])).toBe(0b1010);
    expect(lanesValue("list", [3, 1, 3])).toEqual([1, 3]);
    expect(lanesValue("one", [])).toBeNull();
  });

  it("lists sections in order of first use", () => {
    const form = [
      field("a", { type: "toggle" }, { section: "More" }),
      field("b", { type: "toggle" }),
      field("c", { type: "toggle" }, { section: "Track" }),
      field("d", { type: "toggle" }, { section: "More" }),
    ];
    expect(sections(form)).toEqual(["More", "Track"]);
  });
});
