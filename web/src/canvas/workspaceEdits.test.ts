import { describe, expect, it } from "vitest";
import type { WorkspaceDetail, WorkspaceSnapshot } from "../lib/types";
import { newer, WorkspaceEdits } from "./workspaceEdits";

function detail(revision: number, region: string): WorkspaceDetail {
  return {
    id: 1,
    name: "W",
    created_at: "",
    updated_at: "",
    revision,
    nodes: 0,
    snapshot: {
      version: 2,
      graph: { nodes: [], edges: [] },
      rack: {},
      settings: { band_region: region },
    },
  };
}

function ruler(on: boolean) {
  return (snapshot: WorkspaceSnapshot): WorkspaceSnapshot => ({
    ...snapshot,
    settings: { ...snapshot.settings, band_ruler: on },
  });
}

describe("workspace edits", () => {
  it("replays pending edits over whatever the server says now", () => {
    const edits = new WorkspaceEdits();
    edits.push(1, ruler(true));
    const shown = edits.view(detail(3, "eu"));
    expect(shown.snapshot.settings).toEqual({ band_region: "eu", band_ruler: true });
    const rebased = edits.view(detail(4, "us"));
    expect(rebased.snapshot.settings).toEqual({ band_region: "us", band_ruler: true });
  });

  it("sends against the revision it was built on and holds that base while in flight", () => {
    const edits = new WorkspaceEdits();
    edits.push(1, ruler(true));
    const outgoing = edits.take(1, detail(3, "eu"));
    expect(outgoing?.revision).toBe(3);
    expect(outgoing?.snapshot.settings?.band_ruler).toBe(true);
    expect(edits.take(1, detail(3, "eu"))).toBeNull();

    const echo = edits.view(detail(4, "eu"));
    expect(echo.revision).toBe(3);
    edits.landed(1);
    expect(edits.view(detail(4, "eu")).revision).toBe(4);
  });

  it("keeps edits made during a flight for the next send", () => {
    const edits = new WorkspaceEdits();
    edits.push(1, ruler(true));
    edits.take(1, detail(3, "eu"));
    edits.push(1, ruler(false));
    expect(edits.waiting(1)).toBe(true);
    edits.landed(1);
    expect(edits.take(1, detail(4, "eu"))?.snapshot.settings?.band_ruler).toBe(false);
  });

  it("returns the same object until something changes", () => {
    const edits = new WorkspaceEdits();
    const base = detail(3, "eu");
    expect(edits.view(base)).toBe(base);
    edits.push(1, ruler(true));
    const first = edits.view(base);
    expect(edits.view(base)).toBe(first);
    edits.push(1, ruler(false));
    expect(edits.view(base)).not.toBe(first);
  });

  it("never lets an older answer replace a newer one", () => {
    expect(newer(detail(5, "a"), detail(4, "b")).revision).toBe(5);
    expect(newer(detail(4, "a"), detail(5, "b")).revision).toBe(5);
    expect(newer(undefined, detail(1, "b")).revision).toBe(1);
  });
});
