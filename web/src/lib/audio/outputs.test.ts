import { afterEach, describe, expect, it, vi } from "vitest";
import { listOutputs } from "./outputs";

function devices(list: { kind: string; deviceId: string; label: string }[]): void {
  vi.stubGlobal("navigator", {
    mediaDevices: { enumerateDevices: () => Promise.resolve(list) },
  });
}

describe("listOutputs", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("lists named outputs and leaves the system aliases to System", async () => {
    devices([
      { kind: "audiooutput", deviceId: "default", label: "Default" },
      { kind: "audiooutput", deviceId: "communications", label: "Communications" },
      { kind: "audioinput", deviceId: "mic", label: "Mic" },
      { kind: "audiooutput", deviceId: "a", label: "js8" },
    ]);
    expect(await listOutputs()).toEqual({ outputs: [{ id: "a", name: "js8" }], named: true });
  });

  it("says when the browser hides device names", async () => {
    devices([{ kind: "audiooutput", deviceId: "a", label: "" }]);
    expect(await listOutputs()).toEqual({ outputs: [], named: false });
  });
});
