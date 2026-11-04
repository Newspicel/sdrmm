import { describe, expect, it } from "vitest";
import type { ChannelDescriptor } from "../../lib/types";
import {
  activePreset,
  choiceSummary,
  enabledKinds,
  protocolGroups,
  protocolPresets,
  setEnabled,
} from "./protocols";

function type(
  type_id: string,
  name: string,
  family: ChannelDescriptor["family"],
  identifiable = true,
): ChannelDescriptor {
  return { type_id, name, family, identifiable, bandwidth_hz: 0, input_rate_hz: 0 };
}

const TYPES = [
  type("voice", "Voice", "analog_voice"),
  type("broad", "Broad (wide)", "analog_voice"),
  type("digital", "Digital", "digital_voice"),
  type("plane", "ADS-B (1090ES)", "aviation"),
  type("ident", "Signal identifier", "utility", false),
];

const GROUPS = protocolGroups(TYPES);

describe("protocol groups", () => {
  it("lists only identifiable protocols by family with short labels", () => {
    expect(GROUPS.map((group) => group.family)).toEqual([
      "analog_voice",
      "digital_voice",
      "aviation",
    ]);
    expect(GROUPS[0]?.protocols.map((protocol) => protocol.label)).toEqual(["Voice", "Broad"]);
    expect(GROUPS[2]?.protocols[0]?.name).toBe("ADS-B (1090ES)");
  });

  it("starts with everything enabled", () => {
    const choice = { disabled: [], unidentified: true };
    expect(enabledKinds(GROUPS, choice.disabled)).toEqual(["voice", "broad", "digital", "plane"]);
    expect(activePreset(GROUPS, choice)?.id).toBe("all");
    expect(choiceSummary(GROUPS, choice)).toBe("All protocols");
  });

  it("offers one preset per family that keeps only that family", () => {
    const analog = protocolPresets(GROUPS).find((preset) => preset.id === "analog_voice");
    expect(analog?.choice).toEqual({ disabled: ["digital", "plane"], unidentified: false });
    expect(choiceSummary(GROUPS, { disabled: ["plane", "digital"], unidentified: false })).toBe(
      "Analog voice",
    );
  });

  it("counts a custom choice", () => {
    const disabled = setEnabled([], ["digital"], false);
    expect(disabled).toEqual(["digital"]);
    expect(activePreset(GROUPS, { disabled, unidentified: true })).toBeUndefined();
    expect(choiceSummary(GROUPS, { disabled, unidentified: true })).toBe("3 of 4");
    expect(setEnabled(disabled, ["digital"], true)).toEqual([]);
  });

  it("names an empty choice", () => {
    const disabled = ["voice", "broad", "digital", "plane"];
    expect(choiceSummary(GROUPS, { disabled, unidentified: false })).toBe("Nothing");
    expect(choiceSummary(GROUPS, { disabled, unidentified: true })).toBe("Unidentified only");
  });
});
