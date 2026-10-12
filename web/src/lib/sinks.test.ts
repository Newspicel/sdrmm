import { afterEach, describe, expect, it } from "vitest";
import { sinkEntity, sinkRecord } from "../test/sinks";
import {
  EMPTY_SINK,
  groupsOf,
  SINK_RECORDS,
  SinkHub,
  type SinkSocket,
  snapshotView,
  updatedView,
  useSinkStore,
} from "./sinks";
import type { ClientCommand, DecoderEvent } from "./types";

const PLANE: DecoderEvent = { kind: "adsb", data: { icao: "3c6444", df: 17, raw: "8d" } };
const RTTY: DecoderEvent = { kind: "rtty", data: { text: "CQ" } };

function plane(source: string) {
  return sinkEntity(source, "adsb", "3C6444", PLANE, { summary: "DLH123" });
}

describe("sink feeds", () => {
  afterEach(() => useSinkStore.getState().reset());

  it("keeps the newest record first and applies upserts and removals", () => {
    const view = snapshotView({
      entities: [plane("north")],
      records: [
        sinkRecord("rtty", RTTY, { summary: "1" }),
        sinkRecord("rtty", RTTY, { summary: "2" }),
      ],
      tracks: [],
      surfaces: [],
      lost: 0,
    });
    expect(view.records.map((record) => record.view?.summary)).toEqual(["2", "1"]);

    const next = updatedView(view, {
      upserts: [plane("south")],
      removed: [plane("north").key],
      records: [sinkRecord("rtty", RTTY, { summary: "3" })],
      lost: 2,
    });

    expect(next.entities.map((entity) => entity.key.source)).toEqual(["south"]);
    expect(next.records[0]?.view?.summary).toBe("3");
    expect(next.lost).toBe(2);
  });

  it("caps its records", () => {
    const many = Array.from({ length: SINK_RECORDS + 5 }, (_, n) =>
      sinkRecord("rtty", RTTY, { summary: String(n) }),
    );
    expect(updatedView(EMPTY_SINK, { records: many, lost: 0 }).records).toHaveLength(SINK_RECORDS);
  });

  it("groups by source and kind", () => {
    const groups = groupsOf({
      ...EMPTY_SINK,
      entities: [plane("north"), plane("south")],
      records: [sinkRecord("rtty", RTTY, { summary: "CQ" })],
    });
    expect(groups.map((group) => `${group.source}/${group.kind}`)).toEqual([
      "north/adsb",
      "rtty/rtty",
      "south/adsb",
    ]);
  });

  it("subscribes a sink once while anything watches it", () => {
    const sent: ClientCommand[] = [];
    const socket: SinkSocket = {
      send: (command) => sent.push(command),
      isConnected: () => true,
      on: () => () => {},
    };
    const hub = new SinkHub();
    hub.attach(socket);

    const first = hub.watch("map");
    const second = hub.watch("map");
    first();
    expect(sent).toEqual([{ type: "SubscribeSink", data: { node: "map" } }]);
    second();
    expect(sent.at(-1)).toEqual({ type: "UnsubscribeSink", data: { node: "map" } });
  });

  it("stores snapshots and updates per node", () => {
    const store = useSinkStore.getState();
    store.observe({
      type: "SinkSnapshot",
      data: {
        node: "map",
        feed: { entities: [plane("north")], records: [], tracks: [], surfaces: [], lost: 0 },
      },
    });
    store.observe({
      type: "SinkUpdate",
      data: { node: "readout", update: { upserts: [plane("south")], lost: 0 } },
    });

    expect(useSinkStore.getState().sinks.map?.entities).toHaveLength(1);
    expect(useSinkStore.getState().sinks.readout?.entities[0]?.key.source).toBe("south");
  });
});
