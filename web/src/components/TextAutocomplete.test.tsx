import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { suggestionItem, TextAutocomplete } from "./TextAutocomplete";

describe("suggestionItem", () => {
  it("keys each suggestion by its value, so the highlight follows the row", () => {
    const keys = [{ value: "NAA" }, { value: "GQD" }].map(
      (suggestion) => suggestionItem(suggestion, "NAA").key,
    );

    expect(keys).toEqual(["NAA", "GQD"]);
  });

  it("fills the field with the value, not the detail under it", () => {
    const item = suggestionItem({ value: "NAA", detail: "Cutler" }, "");

    expect(item.props).toMatchObject({ value: "NAA", label: "NAA" });
  });
});

describe("TextAutocomplete", () => {
  it("is a named combobox holding the committed text", () => {
    const html = renderToStaticMarkup(
      <TextAutocomplete label="Call" value="NAA" suggestions={[]} onCommit={() => true} />,
    );

    expect(html).toMatch(/<input[^>]*role="combobox"/);
    expect(html).toContain('aria-label="Call"');
    expect(html).toContain('value="NAA"');
  });
});
