import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { patchCatalogQuery } from "../lib/api";
import { EMPTY_SINK, type SinkView } from "../lib/sinks";
import type {
  Capability,
  CapturedImage,
  DecoderEvent,
  EventKindEntry,
  Field,
  PatchCatalog,
} from "../lib/types";
import { CATALOG } from "../test/catalog";
import { sinkEntity, sinkRecord } from "../test/sinks";
import { ReadoutView } from "./ReadoutView";

const NOW = Date.parse("2026-09-21T10:00:05Z");

function kind(name: string, caps: Capability[]): EventKindEntry {
  return { kind: name, name, noun: name, caps, hue: "teal", lifetime: { type: "instant" } };
}

const KINDS: EventKindEntry[] = [
  kind("probe_station", ["log", "station"]),
  kind("probe_text", ["log", "text"]),
  kind("probe_picture", ["log", "picture"]),
  kind("probe_plain", ["log"]),
];

function event(name: string): DecoderEvent {
  return { kind: name, data: {} };
}

function text(label: string, value: string): Field {
  return { label, value: { type: "text", value } };
}

function line(chunk: string, at: string) {
  return sinkRecord(
    "probe",
    event("probe_text"),
    { summary: chunk, text: chunk, fields: [text("Speed", "22 WPM")] },
    at,
  );
}

function render(feed: SinkView): string {
  const client = new QueryClient();
  const catalog: PatchCatalog = { ...CATALOG, event_kinds: [...CATALOG.event_kinds, ...KINDS] };
  client.setQueryData(patchCatalogQuery().queryKey, catalog);
  return renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <ReadoutView feed={feed} sourceName={(source) => source} now={NOW} />
    </QueryClientProvider>,
  );
}

describe("readout", () => {
  it("tabulates stations with a column per field label, in order of first appearance", () => {
    const html = render({
      ...EMPTY_SINK,
      entities: [
        sinkEntity("probe", "probe_station", "A1", event("probe_station"), {
          summary: "A1 high",
          label: "ALPHA",
          fields: [text("Altitude", "37000 ft"), text("Speed", "451 kt")],
        }),
        sinkEntity("probe", "probe_station", "B2", event("probe_station"), {
          summary: "B2 parked",
          fields: [text("Squawk", "7000")],
        }),
      ],
    });
    const heads = [...html.matchAll(/<th[^>]*>([^<]*)<\/th>/g)].map((match) => match[1]);
    expect(heads).toEqual(["Name", "Altitude", "Speed", "Squawk", "Age"]);
    expect(html).toContain("ALPHA");
    expect(html).toContain("B2");
    expect(html).toContain("37000 ft");
    expect(html).toContain("7000");
    expect(html).toContain("5s");
  });

  it("shows the summary when no station carries fields", () => {
    const html = render({
      ...EMPTY_SINK,
      entities: [
        sinkEntity("probe", "probe_station", "C3", event("probe_station"), {
          summary: "C3: position",
        }),
      ],
    });
    expect(html).toContain("Last");
    expect(html).toContain("C3: position");
  });

  it("joins the text of a text kind and shows its newest fields", () => {
    const html = render({
      ...EMPTY_SINK,
      records: [line("DE DL1ABC", "2026-09-21T10:00:02Z"), line("CQ CQ ", "2026-09-21T10:00:01Z")],
    });
    expect(html).toContain("CQ CQ DE DL1ABC");
    expect(html).toContain("22 WPM");
    expect(html).toContain("Copy all");
  });

  it("shows the pictures a picture kind's decoder captured", () => {
    const picture: CapturedImage = {
      id: 7,
      node: "probe",
      source: "probe",
      mode: "Probe 36",
      device_set: 1,
      channel: 1,
      at: "2026-09-21T10:00:00Z",
      freq_hz: 145_000_000,
      width: 320,
      height: 240,
      lines: 240,
      complete: true,
      image: { url: "/api/images/7.png", media_type: "image/png" },
    };
    const html = render({
      ...EMPTY_SINK,
      pictures: [picture, { ...picture, id: 8, node: "elsewhere" }],
      records: [sinkRecord("probe", event("probe_picture"), { summary: "picture" })],
    });
    expect(html).toContain("<img");
    expect(html).toContain("Probe 36 320 × 240");
    expect(html.match(/<img/g)).toHaveLength(1);
  });

  it("shows the latest record of any other kind", () => {
    const html = render({
      ...EMPTY_SINK,
      records: [
        sinkRecord("probe", event("probe_plain"), {
          summary: "Station 10:00",
          fields: [text("Station", "TEST")],
        }),
      ],
    });
    expect(html).toContain("Station 10:00");
    expect(html).toContain("TEST");
  });

  it("names each group when a feed holds several", () => {
    const html = render({
      ...EMPTY_SINK,
      records: [
        sinkRecord("probe", event("probe_plain"), { summary: "ALERT 1" }),
        line("CQ", "2026-09-21T10:00:01Z"),
      ],
    });
    expect(html).toContain("probe · probe_plain");
    expect(html).toContain("probe · probe_text");
  });
});
