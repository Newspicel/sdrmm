import { describe, expect, it, vi } from "vitest";
import {
  networkExportControlsLocked,
  networkExportMutationOptions,
} from "../../components/networkExport";
import type { NetworkExportSettings, NetworkExportStatus } from "../../lib/types";

const settings: NetworkExportSettings = {
  transport: "tcp",
  format: "ci16_le",
  address: "analysis.local:7355",
};

const status: NetworkExportStatus = {
  node: "net-a",
  stream: 2,
  settings,
  sample_rate: 2_400_000,
  center_hz: 100_000_000,
  samples: 10,
  bytes: 40,
  packets: 1,
  overruns: 0,
};

describe("NetworkExportFace lifecycle", () => {
  it("sends start and stop to the node, which the server resolves to its source", async () => {
    const request = vi.fn(async () => ({ result: "network_export" as const, with: status }));
    const options = networkExportMutationOptions("net-a", request);

    await options.mutationFn("start");
    await options.mutationFn("stop");

    expect(request).toHaveBeenNthCalledWith(1, "net-a", {
      action: "network_export",
      with: "start",
    });
    expect(request).toHaveBeenNthCalledWith(2, "net-a", { action: "network_export", with: "stop" });
  });

  it("locks settings during a request and throughout an active export", () => {
    expect(networkExportControlsLocked({ kind: "ready" }, false)).toBe(false);
    expect(networkExportControlsLocked({ kind: "ready" }, true)).toBe(true);
    expect(networkExportControlsLocked({ kind: "active", status }, false)).toBe(true);
  });

  it("routes request failures to the toast callback", () => {
    const notify = vi.fn();
    const options = networkExportMutationOptions("net-a", undefined, notify);
    options.onError(new Error("destination refused"));
    expect(notify).toHaveBeenCalledWith("destination refused");
  });
});
