import { createElement } from "react";
import { describe, expect, it } from "vitest";
import { advance, gone, heldFirst, shown } from "./presence";

const row = (key: string) => createElement("div", { key });

describe("presence", () => {
  it("shows the first rows without entering them", () => {
    const first = [row("rx0"), row("tx")];
    const held = heldFirst(first);
    expect(shown(first, held).map((r) => [r.key, r.leaving])).toEqual([
      ["rx0", false],
      ["tx", false],
    ]);
    expect(held.entered.size).toBe(0);
  });

  it("enters a new row and keeps a dropped one in place until it is gone", () => {
    const first = [row("all"), row("rx0"), row("rx1"), row("tx")];
    const after = [row("rx0"), row("tx")];
    const held = advance(heldFirst(first), after);
    expect(shown(after, held).map((r) => [r.key, r.leaving])).toEqual([
      ["all", true],
      ["rx0", false],
      ["rx1", true],
      ["tx", false],
    ]);
    const settled = gone(gone(held, "all"), "rx1");
    expect(shown(after, settled).map((r) => r.key)).toEqual(["rx0", "tx"]);

    const back = advance(settled, first);
    expect([...back.entered]).toEqual(["all", "rx1"]);
  });

  it("takes back a row that returns while it was leaving", () => {
    const first = [row("rx0"), row("rx1")];
    const leaving = advance(heldFirst(first), [row("rx0")]);
    const back = advance(leaving, first);
    expect(back.leaving.size).toBe(0);
  });
});
