import { describe, expect, it } from "vitest";
import type { DeviceInfo } from "../../lib/types";
import { deviceRefOf } from "./deviceNode";

function info(overrides: Partial<DeviceInfo>): DeviceInfo {
  return { driver: "rtlsdr", key: "0", label: "RTL-SDR", ...overrides };
}

describe("device references", () => {
  it("prefers the serial, falls back to the key", () => {
    expect(deviceRefOf(info({ serial: "00000001" }))).toEqual({
      backend: "rtlsdr",
      serial: "00000001",
    });
    expect(
      deviceRefOf(
        info({ driver: "soapy", key: "123456@DT", label: "Dual Tuner", serial: "123456" }),
      ),
    ).toEqual({ backend: "soapy", serial: "123456", key: "123456@DT" });
    expect(deviceRefOf(info({ driver: "virtual", key: "file:/rec/capture" }))).toEqual({
      backend: "virtual",
      key: "file:/rec/capture",
    });
  });
});
