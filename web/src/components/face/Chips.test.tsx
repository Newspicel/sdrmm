import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ChipField, ChipRows, ChoiceChip, SettingChip, ToggleChip } from "./Chips";

const OPTIONS = [
  { value: "fm", label: "FM" },
  { value: "am", label: "AM" },
] as const;

function rows(children: ReactNode): string {
  return renderToStaticMarkup(<ChipRows>{children}</ChipRows>);
}

const choice = (
  <ChoiceChip label="Mode" value="fm" options={OPTIONS} title="Mode" onChange={() => {}} />
);

describe("ChipRows", () => {
  it("draws a choice as a dropdown", () => {
    expect(renderToStaticMarkup(choice)).toContain('aria-haspopup="dialog"');
    expect(rows(choice)).toContain('role="combobox"');
  });

  it("draws on/off as a Yes/No dropdown", () => {
    const toggle = <ToggleChip label="AGC" on title="AGC" onChange={() => {}} />;
    const html = rows(toggle);
    expect(html).toContain('role="combobox"');
    expect(html).toContain(">Yes<");
    expect(html).not.toContain('role="switch"');
  });

  it("opens a setting in place", () => {
    const chip = (
      <SettingChip label="IF" value="off" title="IF filter">
        {() => <ChipField label="Edges">inside</ChipField>}
      </SettingChip>
    );
    expect(renderToStaticMarkup(chip)).not.toContain("inside");
    expect(rows(chip)).toContain("inside");
  });

  it("lists what a running decoder found in place", () => {
    const services = Array.from({ length: 8 }, (_, index) => ({
      value: index,
      label: `Service ${index}`,
    }));
    const html = rows(
      <ChoiceChip
        label="Service"
        value={3}
        options={services}
        title="Service"
        listed
        onChange={() => {}}
      />,
    );
    expect(html).toContain('role="listbox"');
    expect(html).toMatch(/aria-selected="true"[^>]*>Service 3</);
  });
});
