import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { IfFilter } from "../lib/types";
import { IfFilterChip } from "./IfFilterControl";

function render(filter: IfFilter): string {
  return renderToStaticMarkup(
    <IfFilterChip
      filter={filter}
      own={{ low_hz: -100_000, high_hz: 100_000 }}
      channelHz={99_900_000}
      onFilter={() => undefined}
    />,
  );
}

describe("IfFilterChip", () => {
  it("reads off until enabled, then on until it shapes something", () => {
    expect(render({})).toContain(">off<");
    expect(render({ enabled: true })).toContain(">on<");
  });

  it("shows the edges and the notch count when on", () => {
    const html = render({
      enabled: true,
      low_hz: -90_000,
      notches: [{ frequency_hz: 99_930_000, width_hz: 2_000 }],
    });
    expect(html).toContain("−90 kHz +100 kHz, 1 notch");
  });
});
