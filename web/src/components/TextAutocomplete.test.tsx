import { describe, expect, it } from "vitest";
import { suggestionItem } from "./TextAutocomplete";

describe("suggestionItem", () => {
  it("keys each suggestion by its value, so the highlight follows the row", () => {
    const keys = [{ value: "NAA" }, { value: "GQD" }].map(
      (suggestion) => suggestionItem(suggestion, "NAA").key,
    );

    expect(keys).toEqual(["NAA", "GQD"]);
  });
});
