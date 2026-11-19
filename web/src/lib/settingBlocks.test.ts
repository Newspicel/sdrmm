import { describe, expect, it } from "vitest";
import { needsOf, settingBlocks } from "./settingBlocks";
import type { SettingField } from "./types";

const TONE: SettingField = {
  key: "tone",
  label: "Tone",
  control: {
    type: "choice",
    options: [
      { value: "off", label: "off" },
      { value: "ctcss", label: "CTCSS" },
      { value: "dcs", label: "DCS" },
    ],
  },
};
const FORM: SettingField[] = [
  { key: "bw", label: "BW", control: { type: "number" } },
  TONE,
  {
    key: "ctcss",
    label: "CTCSS",
    when: [{ key: "tone", any_of: ["ctcss"] }],
    control: { type: "number" },
  },
  { key: "emphasis", label: "Emphasis", control: { type: "toggle" } },
  {
    key: "dcs",
    label: "DCS",
    when: [{ key: "tone", any_of: ["dcs"] }],
    control: { type: "number" },
  },
];

describe("settingBlocks", () => {
  it("puts each setting right under the one it needs", () => {
    expect(settingBlocks(FORM)).toEqual([[0], [1, 2, 4], [3]]);
  });

  it("leaves a field whose parent is elsewhere as its own block", () => {
    const loose: SettingField = { ...FORM[2], when: [{ key: "gone", any_of: [] }] } as SettingField;
    expect(settingBlocks([loose])).toEqual([[0]]);
  });
});

describe("needsOf", () => {
  it("names the setting and the value that turns a field on", () => {
    expect(needsOf(FORM, { tone: "off" }, FORM[2] as SettingField)).toEqual({
      key: "tone",
      hint: "Set Tone to CTCSS",
    });
  });

  it("asks for nothing once the field applies", () => {
    expect(needsOf(FORM, { tone: "ctcss" }, FORM[2] as SettingField)).toBeNull();
  });
});
