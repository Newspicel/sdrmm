import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SearchableSelect } from "./SearchableSelect";
import { Select } from "./Select";

const RATES = [
  { value: 48, label: "48 kS/s" },
  { value: 240, label: "240 kS/s", title: "Default" },
  { value: 2000, label: "2 MS/s", disabled: true },
] as const;

function trigger(html: string): string {
  return /<button[^>]*role="combobox"[^>]*>.*?<\/button>/.exec(html)?.[0] ?? "";
}

describe("Select", () => {
  it("is a combobox named by its label that shows the chosen option", () => {
    const html = renderToStaticMarkup(
      <Select label="Rate" value={240} options={RATES} onChange={() => {}} />,
    );
    expect(trigger(html)).toContain('aria-label="Rate"');
    expect(trigger(html)).toMatch(/>240 kS\/s</);
  });

  it("can be disabled", () => {
    const html = renderToStaticMarkup(
      <Select label="Rate" value={48} options={RATES} onChange={() => {}} disabled />,
    );
    expect(trigger(html)).toMatch(/\sdisabled=""/);
  });
});

describe("SearchableSelect", () => {
  it("shows the chosen option before the list ever opens", () => {
    const html = renderToStaticMarkup(
      <SearchableSelect label="Signal" value={48} options={RATES} onChange={() => {}} />,
    );
    expect(trigger(html)).toContain('aria-label="Signal"');
    expect(trigger(html)).toMatch(/>48 kS\/s</);
  });
});
