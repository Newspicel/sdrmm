import { describe, expect, it } from "vitest";
import { switchNotice, useSwitchStore } from "./switches";

const report = { bound: [], opened: 0, created: 0 };

describe("switches", () => {
  it("knows a switch this browser asked for", () => {
    const store = useSwitchStore.getState();
    store.expect(4);
    const mine = store.observe({ type: "WorkspaceSwitched", data: { id: 4, report } });
    expect(mine).toEqual({ id: 4, by: null, mine: true });
    const theirs = useSwitchStore
      .getState()
      .observe({ type: "WorkspaceSwitched", data: { id: 4, by: "Ann", report } });
    expect(theirs?.mine).toBe(false);
    expect(useSwitchStore.getState().report).toBe(report);
  });

  it("says who switched and where", () => {
    const list = {
      workspaces: [
        { id: 4, name: "Marine", created_at: "", updated_at: "", revision: 1, nodes: 0 },
      ],
    };
    expect(switchNotice({ id: 4, by: "Ann", mine: false }, list)).toBe("Ann switched to Marine");
    expect(switchNotice({ id: 9, by: null, mine: false }, undefined)).toBe(
      "Someone switched to another workspace",
    );
  });
});
