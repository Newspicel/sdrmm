import { describe, expect, it } from "vitest";
import type { TemplateInfo } from "../lib/types";
import { templateSize } from "./templates";

function template(extra: Partial<TemplateInfo>): TemplateInfo {
  return {
    id: "t",
    name: "T",
    description: "",
    explainer: "",
    center_hz: 100e6,
    sample_rate: 2.4e6,
    channels: [],
    min_freq_hz: 100e6,
    max_freq_hz: 100e6,
    ...extra,
  };
}

describe("templateSize", () => {
  it("counts channels", () => {
    expect(templateSize(template({}))).toBe("0 ch");
    expect(
      templateSize(
        template({
          channels: [
            { frequency_hz: 100e6, params: { type: "nfm", settings: {} } },
          ] as TemplateInfo["channels"],
        }),
      ),
    ).toBe("1 ch");
  });
});
