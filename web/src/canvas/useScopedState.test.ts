import { describe, expect, it } from "vitest";
import { scopedValue } from "./useScopedState";

describe("scoped state", () => {
  it("forgets the value when the workspace changes", () => {
    const held = { scope: 1, value: "scope" };
    expect(scopedValue(held, 1, null)).toBe("scope");
    expect(scopedValue(held, 2, null)).toBeNull();
  });
});
