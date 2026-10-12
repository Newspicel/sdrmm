import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PANELS } from "../components/DecoderPanels";
import { patchCatalogQuery } from "../lib/api";
import { EMPTY_SINK, type SinkView } from "../lib/sinks";
import type { DecoderEvent } from "../lib/types";
import { CATALOG } from "../test/catalog";
import { sinkEntity, sinkRecord } from "../test/sinks";
import type { Panels } from "./panels";
import { ReadoutView } from "./ReadoutView";

const NOW = Date.parse("2026-09-21T10:00:05Z");
const SHIP: DecoderEvent = {
  kind: "aprs",
  data: { source: "DL1ABC", destination: "APRS", path: [], info: "" },
} as never;

function rtty(text: string, at: string) {
  return sinkRecord("rtty", { kind: "rtty", data: { text } }, { summary: text, text }, at);
}

function render(feed: SinkView, panels: Panels = {}): string {
  const client = new QueryClient();
  client.setQueryData(patchCatalogQuery().queryKey, CATALOG);
  return renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <ReadoutView feed={feed} panels={panels} sourceName={(source) => source} now={NOW} />
    </QueryClientProvider>,
  );
}

describe("readout", () => {
  it("lists station entities in a generic table", () => {
    const html = render({
      ...EMPTY_SINK,
      entities: [
        sinkEntity("aprs", "aprs", "DL1ABC", SHIP, {
          summary: "DL1ABC: position",
          label: "DL1ABC-9",
        }),
      ],
    });
    expect(html).toContain("DL1ABC-9");
    expect(html).toContain("DL1ABC: position");
    expect(html).toContain("5s");
  });

  it("shows the latest record of a kind without a station", () => {
    const html = render({
      ...EMPTY_SINK,
      records: [
        sinkRecord("clock", { kind: "radio_clock", data: {} } as never, {
          summary: "DCF77 10:00",
          fields: [{ label: "Station", value: { type: "text", value: "DCF77" } }],
        }),
      ],
    });
    expect(html).toContain("DCF77 10:00");
    expect(html).toContain("Station");
  });

  it("joins the text of a text kind", () => {
    const html = render({
      ...EMPTY_SINK,
      records: [rtty("DE DL1ABC", "2026-09-21T10:00:02Z"), rtty("CQ CQ ", "2026-09-21T10:00:01Z")],
    });
    expect(html).toContain("CQ CQ DE DL1ABC");
  });

  it("renders a panel from its props", () => {
    const html = render(
      {
        ...EMPTY_SINK,
        entities: [
          sinkEntity(
            "adsb",
            "adsb",
            "3C6444",
            { kind: "adsb", data: { icao: "3c6444", df: 17, raw: "8d", callsign: "DLH123" } },
            { summary: "DLH123" },
          ),
        ],
      },
      PANELS,
    );
    expect(html).toContain("Aircraft");
    expect(html).toContain("DLH123");
  });

  it("shows the broadcast status a channel sent down its wire", () => {
    const html = render(
      {
        ...EMPTY_SINK,
        records: [
          sinkRecord(
            "dab",
            {
              kind: "broadcast",
              data: {
                system: "dab_plus",
                locked: true,
                snr_db: 18,
                frequency_error_hz: 0,
                label: "Radio Eins",
                dynamic_label: "Now playing",
              },
            },
            { summary: "Radio Eins locked" },
          ),
        ],
      },
      PANELS,
    );
    expect(html).toContain("Radio Eins");
    expect(html).toContain("Now playing");
  });

  it("shows only what its own feed holds, so a filter upstream decides", () => {
    const html = render({
      ...EMPTY_SINK,
      records: [sinkRecord("pager", { kind: "pocsag", data: {} } as never, { summary: "ALERT 1" })],
    });
    expect(html).toContain("ALERT 1");
    expect(html).not.toContain("DLH123");
  });
});
