import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Form, Settings } from "../lib/settingsForm";
import type { Control, Field, ParamChoice, SettingField } from "../lib/types";
import { enteredHz } from "./HzField";
import { listShown } from "./ListChip";
import { SettingsForm } from "./SettingsForm";

type Choice = Extract<Control, { type: "choice" }>;

function field(
  key: string,
  control: SettingField["control"],
  over: Partial<SettingField> = {},
): SettingField {
  return { key, label: key, control, ...over };
}

function render(
  form: Form,
  settings: Settings,
  choices: ParamChoice[] = [],
  readouts: Field[] = [],
): string {
  return renderToStaticMarkup(
    <SettingsForm
      form={form}
      settings={settings}
      choices={choices}
      readouts={readouts}
      onSettings={() => {}}
    />,
  );
}

const MODES: Choice = {
  type: "choice",
  options: [
    { value: "off", label: "Off" },
    { value: "tone", label: "Tone" },
  ],
};

describe("SettingsForm", () => {
  it("hides a field until its condition holds", () => {
    const form: Form = [
      field("mode", MODES, { label: "Mode" }),
      field(
        "tone_hz",
        { type: "number", unit: "Hz" },
        {
          label: "Tone",
          when: [{ key: "mode", any_of: ["tone"] }],
          fallback: 88.5,
        },
      ),
    ];
    expect(render(form, { mode: "off" })).not.toContain("88.5 Hz");
    expect(render(form, { mode: "tone" })).toContain("88.5 Hz");
  });

  it("shows hertz with their prefix", () => {
    const form = [field("bandwidth_hz", { type: "number", unit: "Hz" }, { label: "BW" })];
    expect(render(form, { bandwidth_hz: 12_500 })).toContain("12.5 kHz");
  });

  it("mutes a field whose value is quiet", () => {
    const form = [field("mode", MODES, { label: "Mode", quiet: ["off"] })];
    expect(render(form, { mode: "off" })).toContain("font-normal text-ink-faint");
    expect(render(form, { mode: "tone" })).not.toContain("font-normal text-ink-faint");
  });

  it("shows a current value missing from the options", () => {
    expect(render([field("mode", MODES, { label: "Mode" })], { mode: "squelch" })).toContain(
      "squelch",
    );
  });

  it("offers live options once the status publishes them", () => {
    const form = [
      field(
        "service",
        { type: "live", fallback: { type: "number", optional: "Auto" } },
        { label: "Service" },
      ),
    ];
    expect(render(form, { service: null })).toContain("Auto");
    const choices = [{ name: "service", options: [{ value: 7, label: "Radio Seven" }] }];
    expect(render(form, { service: 7 }, choices)).toContain("Radio Seven");
  });

  it("masks a secret text", () => {
    const form = [field("key", { type: "text", secret: true }, { label: "Key" })];
    const html = render(form, { key: "00112233" });
    expect(html).toContain("set");
    expect(html).not.toContain("00112233");
  });

  it("names the matching preset, else Custom", () => {
    const form = [
      field(
        "preset",
        {
          type: "preset",
          options: [{ label: "Slow", values: { rate: 12 } }],
        },
        { label: "Preset" },
      ),
    ];
    expect(render(form, { rate: 12 })).toContain("Slow");
    expect(render(form, { rate: 9 })).toContain("Custom");
  });

  it("counts list entries, or names the empty list", () => {
    const form = [
      field(
        "keys",
        { type: "list", item: [field("name", { type: "text" })], empty: "built-in" },
        { label: "Keys" },
      ),
    ];
    expect(render(form, {})).toContain("built-in");
    expect(render(form, { keys: [{ name: "a" }, { name: "b" }] })).toContain(">2<");
    expect(listShown([], null)).toBe("none");
  });

  it("follows an option with the status readout it names", () => {
    const form = [
      field(
        "mode",
        {
          type: "choice",
          options: [
            { value: null, label: "Auto", status: "mode" },
            { value: 1, label: "TM1" },
          ],
        },
        { label: "Mode" },
      ),
    ];
    const found: Field = { key: "mode", label: "Mode", value: { type: "text", value: "TM2" } };
    expect(render(form, { mode: null }, [], [found])).toContain("Auto TM2");
    expect(render(form, { mode: null })).not.toContain("TM2");
  });

  it("shows a scaled number in its own unit", () => {
    const form = [
      field(
        "freq",
        { type: "number", unit: "MHz", scale: 1e6, min: 24e6, max: 1.7e9 },
        { label: "Freq" },
      ),
    ];
    const html = render(form, { freq: 145_500_000 });
    expect(html).toContain("145.5");
    expect(html).not.toContain("145500000");
  });
});

describe("enteredHz", () => {
  it("accepts SI prefixes inside the field's range", () => {
    expect(enteredHz("2.4 kHz", { min: 0, max: 10_000 })).toBe(2_400);
    expect(enteredHz("20 kHz", { min: 0, max: 10_000 })).toBeUndefined();
    expect(enteredHz("wide", {})).toBeUndefined();
  });

  it("clears an optional field, keeps a required one", () => {
    expect(enteredHz(" ", { optional: true })).toBeNull();
    expect(enteredHz("", {})).toBeUndefined();
  });
});

describe("SettingsForm for array processors", () => {
  it("shows a scaled number in its display unit", () => {
    const form = [
      field(
        "offset_hz",
        { type: "number", min: -1e6, max: 1e6, step: 100, unit: "kHz", scale: 1_000 },
        { label: "Offset" },
      ),
    ];
    expect(render(form, { offset_hz: 12_500 })).toMatch(/>12\.5<.*kHz/);
  });

  it("draws only the fields of the section it is given", () => {
    const form = [
      field("loose", { type: "toggle" }, { label: "Loose" }),
      field("folded", { type: "toggle" }, { label: "Folded", section: "More" }),
    ];
    const loose = renderToStaticMarkup(
      <SettingsForm form={form} settings={{}} choices={[]} section={null} onSettings={() => {}} />,
    );
    expect(loose).toContain(">Loose<");
    expect(loose).not.toContain(">Folded<");
    const folded = renderToStaticMarkup(
      <SettingsForm form={form} settings={{}} choices={[]} section="More" onSettings={() => {}} />,
    );
    expect(folded).toContain(">Folded<");
  });

  it("numbers lanes from one and names an empty set", () => {
    const form = [
      field("main", { type: "lanes", pick: "one" }, { label: "Main" }),
      field("refs", { type: "lanes", pick: "list", empty: "others" }, { label: "Refs" }),
    ];
    const html = render(form, { main: 2, refs: [] });
    expect(html).toMatch(/>Main<\/span><b[^>]*>3</);
    expect(html).toMatch(/>Refs<\/span><b[^>]*>others</);
  });

  it("lists numbers with their unit", () => {
    const form = [
      field(
        "nulls_deg",
        { type: "numbers", spec: { min: 0, max: 359.9, unit: "°" }, max_len: 3 },
        { label: "Nulls" },
      ),
    ];
    expect(render(form, { nulls_deg: [45, 90] })).toMatch(/>Nulls<\/span><b[^>]*>45° 90°</);
    expect(render(form, { nulls_deg: [] })).toMatch(/>Nulls<\/span><b[^>]*>none</);
  });

  it("offers no choice above the field's ceiling", () => {
    const form = [
      field("bins", { type: "number" }, { label: "FFT" }),
      field(
        "columns",
        {
          type: "choice",
          options: [
            { value: 64, label: "64" },
            { value: 512, label: "512" },
          ],
        },
        { label: "Columns", at_most: "bins" },
      ),
    ];
    expect(render(form, { bins: 256, columns: 512 })).toMatch(/>Columns<\/span><b[^>]*>512</);
  });
});
