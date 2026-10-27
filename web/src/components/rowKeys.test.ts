import { describe, expect, it } from "vitest";
import { rowKeys } from "./rowKeys";

describe("rowKeys", () => {
  it("keeps a row's key when the row above it is removed", () => {
    const rows = [{ freq_hz: 1_000 }, { freq_hz: 2_000 }, { freq_hz: 3_000 }];
    const before = rowKeys(rows);
    const after = rowKeys(rows.slice(1));

    expect(after).toEqual(before.slice(1));
  });

  it("tells two identical rows apart", () => {
    const keys = rowKeys([{ meter: "" }, { meter: "" }]);

    expect(new Set(keys).size).toBe(2);
  });

  it("follows a row that moved rather than its place in the list", () => {
    const first = { start_hz: 1, stop_hz: 2, step_hz: 1 };
    const second = { start_hz: 3, stop_hz: 4, step_hz: 1 };

    expect(rowKeys([first, second])[1]).toBe(rowKeys([second, first])[0]);
  });
});
