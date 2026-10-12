import { describe, expect, it } from "vitest";
import type { DeviceInfo, PatchGraph, PatchNode } from "../../lib/types";
import { claimedDevices, deviceRefOf, nodeDeviceRef, refMatches } from "./deviceNode";

function info(overrides: Partial<DeviceInfo>): DeviceInfo {
  return { driver: "rtlsdr", key: "0", label: "RTL-SDR", ...overrides };
}

function node(id: string, body: Partial<PatchNode> & Pick<PatchNode, "kind">): PatchNode {
  return { id, position: { x: 0, y: 0 }, ...body } as PatchNode;
}

describe("device references", () => {
  it("prefers the serial, falls back to the key, and accepts a singleton backend", () => {
    const hardware = info({ serial: "00000001" });
    const bySerial = deviceRefOf(hardware);
    expect(bySerial).toEqual({ backend: "rtlsdr", serial: "00000001" });
    expect(refMatches(bySerial, hardware)).toBe(true);
    expect(refMatches(bySerial, info({ key: "3", serial: "00000001" }))).toBe(true);
    expect(refMatches(bySerial, info({ serial: "00000002" }))).toBe(false);

    const duo = info({
      driver: "soapy",
      key: "123456@DT",
      label: "Dual Tuner",
      serial: "123456",
    });
    const byVariant = deviceRefOf(duo);
    expect(byVariant).toEqual({ backend: "soapy", serial: "123456", key: "123456@DT" });
    expect(refMatches(byVariant, duo)).toBe(true);
    expect(refMatches(byVariant, { ...duo, key: "123456@ST" })).toBe(false);

    const file = info({ driver: "virtual", key: "file:/rec/capture", serial: undefined });
    const byKey = deviceRefOf(file);
    expect(byKey).toEqual({ backend: "virtual", key: "file:/rec/capture" });
    expect(refMatches(byKey, file)).toBe(true);
    expect(refMatches(byKey, info({ driver: "virtual", key: "band" }))).toBe(false);

    const singleton = { backend: "hackrf" };
    expect(refMatches(singleton, info({ driver: "hackrf", key: "0" }))).toBe(true);
    expect(refMatches(singleton, hardware)).toBe(false);
  });
});

describe("claimedDevices", () => {
  it("lists the radios other device nodes name", () => {
    const graph: PatchGraph = {
      nodes: [
        node("a", { kind: "device", data: { device: { backend: "rtlsdr", serial: "A" } } }),
        node("b", { kind: "device", data: { device: null } }),
        node("c", { kind: "device", data: { device: { backend: "hackrf" } } }),
      ],
      edges: [],
    };
    expect(claimedDevices(graph, "c")).toEqual([{ backend: "rtlsdr", serial: "A" }]);
  });
});

describe("nodeDeviceRef", () => {
  it("names the radio a recording or a running generator opens", () => {
    expect(nodeDeviceRef(node("r", { kind: "recording", data: { recording: "take" } }))).toEqual({
      backend: "recording",
      key: "take",
    });
    expect(nodeDeviceRef(node("g:1", { kind: "signal_gen", data: { running: true } }))).toEqual({
      backend: "siggen",
      key: "g-1",
    });
    expect(nodeDeviceRef(node("s", { kind: "scope" }))).toBeNull();
  });
});
