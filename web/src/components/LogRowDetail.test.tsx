import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { LogRow } from "./decoderLog";
import { RowDetail } from "./LogRowDetail";

function row(over: Partial<LogRow> = {}): LogRow {
  return {
    key: "live:1",
    at: "2026-09-21T10:00:00Z",
    kind: "call",
    station: "2620001",
    callsign: null,
    summary: "2620001 → TG 9",
    freqHz: 439_000_000,
    source: "dmr",
    live: true,
    fields: [
      { label: "Talkgroup", value: { type: "text", value: "9" } },
      {
        label: "Duration",
        value: { type: "number", value: { value: 2.5, unit: "seconds", digits: 1 } },
      },
    ],
    clip: null,
    ...over,
  };
}

describe("log row detail", () => {
  it("lists the fields the server presented", () => {
    const html = renderToStaticMarkup(<RowDetail row={row()} />);
    expect(html).toContain("Talkgroup");
    expect(html).toContain("2.5 s");
    expect(html).not.toContain("<audio");
  });

  it("plays a clip only when the view carries one", () => {
    const html = renderToStaticMarkup(
      <RowDetail row={row({ clip: { url: "/api/calls/1/audio", media_type: "audio/wav" } })} />,
    );
    expect(html).toContain("<audio");
  });

  it("says so when a frame carries nothing more", () => {
    const html = renderToStaticMarkup(<RowDetail row={row({ fields: [] })} />);
    expect(html).toContain("nothing beyond its summary");
  });
});
