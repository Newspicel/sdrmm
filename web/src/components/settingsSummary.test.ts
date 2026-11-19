import { describe, expect, it } from "vitest";
import type { SettingField } from "../lib/types";
import { settingsSummary } from "./settingsSummary";

const FORM: SettingField[] = [
  {
    key: "deviation",
    label: "Dev",
    control: { type: "number", unit: "Hz" },
  },
  {
    key: "tone",
    label: "Tone",
    quiet: ["off"],
    control: {
      type: "choice",
      options: [
        { value: "off", label: "off" },
        { value: "ctcss", label: "CTCSS" },
      ],
    },
  },
  { key: "emphasis", label: "Emphasis", control: { type: "toggle" } },
];

describe("settingsSummary", () => {
  it("names what each field is set to", () => {
    expect(settingsSummary(FORM, { deviation: 5000, tone: "ctcss", emphasis: true })).toEqual([
      { key: "deviation", label: "Dev", value: "5 kHz" },
      { key: "tone", label: "Tone", value: "CTCSS" },
      { key: "emphasis", label: "Emphasis", value: "on" },
    ]);
  });

  it("leaves out what is off", () => {
    expect(settingsSummary(FORM, { tone: "off", emphasis: false }).map((row) => row.key)).toEqual(
      [],
    );
  });
});
