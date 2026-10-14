import { afterEach, expect, it } from "vitest";
import { readoutStats, useReadouts } from "./readouts";
import type { Readout } from "./types";

afterEach(() => useReadouts.getState().reset());

const number = (name: string, label: string, value: number, unit: string, stream?: number) =>
  ({ name, label, stream, value: { kind: "number", value, unit } }) as Readout;

it("keeps the latest readouts of each radio", () => {
  const observe = useReadouts.getState().observe;
  observe({
    type: "DeviceReadouts",
    data: { device_set: 3, readouts: [number("temperature", "Chip", 43.86, "°C")] },
  });
  observe({
    type: "DeviceReadouts",
    data: { device_set: 3, readouts: [number("temperature", "Chip", 44.1, "°C")] },
  });
  expect(useReadouts.getState().byDevice.get(3)?.[0]?.value).toEqual({
    kind: "number",
    value: 44.1,
    unit: "°C",
  });
});

it("puts every lane of one readout on one stat and warns on a lost lock", () => {
  const stats = readoutStats([
    number("temperature", "Chip", 43.86, "°C"),
    number("rssi", "RSSI", -86.75, "dB", 0),
    number("rssi", "RSSI", -90.25, "dB", 1),
    { name: "reference", label: "Ref", value: { kind: "flag", on: false } },
  ]);
  expect(stats.map((stat) => [stat.label, stat.text, stat.unit, stat.warn])).toEqual([
    ["Chip", "43.9", "°C", false],
    ["RSSI", "-86.8 / -90.3", "dB", false],
    ["Ref", "unlocked", "", true],
  ]);
});
