import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { SettingField } from "../lib/types";
import { ChipRows } from "./face/Chips";
import { SettingsForm } from "./SettingsForm";

const FORM: SettingField[] = [
  {
    key: "tone",
    label: "Tone",
    control: {
      type: "choice",
      options: [
        { value: "off", label: "off" },
        { value: "ctcss", label: "CTCSS" },
      ],
    },
  },
  {
    key: "ctcss",
    label: "CTCSS",
    when: [{ key: "tone", any_of: ["ctcss"] }],
    control: { type: "number", unit: "Hz" },
  },
];

function rows(tone: string): string {
  return renderToStaticMarkup(
    <ChipRows>
      <SettingsForm form={FORM} settings={{ tone }} choices={[]} onSettings={() => {}} />
    </ChipRows>,
  );
}

describe("SettingsForm in the inspector", () => {
  it("keeps a moot setting in place, greyed, pointing at what turns it on", () => {
    const html = rows("off");
    expect(html).toContain(">CTCSS<");
    expect(html).toContain("<fieldset disabled");
    expect(html).toContain('aria-label="Set Tone to CTCSS"');
    expect(html).toContain('data-setting="tone"');
  });

  it("draws it live once it applies", () => {
    const html = rows("ctcss");
    expect(html).not.toContain("<fieldset disabled");
    expect(html).not.toContain("Set Tone to");
  });
});
