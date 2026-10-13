import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ListenerRegistry } from "./listeners";
import { PortHub } from "./ports";
import {
  resampleRows,
  SPECTRUM_FPS,
  SPECTRUM_HIDDEN_FPS,
  SPECTRUM_HISTORY_ROWS,
  SpectrumHub,
} from "./spectrum";
import type { ClientCommand } from "./types";

function fakeSocket() {
  const sent: ClientCommand[] = [];
  const registry = new ListenerRegistry();
  const socket = {
    send: (command: ClientCommand) => sent.push(command),
    on: registry.on.bind(registry),
  };
  return {
    socket,
    sent,
    started: (streamId: number, node: string, port = "iq") =>
      registry.emit("event", {
        type: "PortStream",
        data: { stream_id: streamId, node, port, source: "radio", kind: "spectrum" },
      }),
    stopped: (streamId: number) =>
      registry.emit("event", {
        type: "StreamStopped",
        data: { stream_id: streamId, kind: "spectrum" },
      }),
    push: (
      streamId: number,
      bins: readonly number[] = [1, 2],
      window: { dbMin: number; dbMax: number } = { dbMin: -110, dbMax: -20 },
    ) =>
      registry.emit("spectrum", {
        streamId,
        seq: 0,
        timestamp: 0n,
        centerHz: 100e6,
        spanHz: 2e6,
        dbMin: window.dbMin,
        floorDb: window.dbMin + 10,
        dbMax: window.dbMax,
        bins: Uint8Array.from(bins),
      }),
    reconnect: () => registry.emit("status", true),
    attached: () => registry.count("spectrum") > 0,
  };
}

type Subscribe = Extract<ClientCommand, { type: "SubscribePort" }>;

const subscribes = (sent: ClientCommand[]): Subscribe[] =>
  sent.filter((c): c is Subscribe => c.type === "SubscribePort");
const unsubscribes = (sent: ClientCommand[]) => sent.filter((c) => c.type === "UnsubscribePort");

function bench(fake = fakeSocket()) {
  const ports = new PortHub();
  ports.attach(fake.socket);
  const hub = new SpectrumHub(ports);
  hub.attach(fake.socket);
  return { fake, hub, ports };
}

const waitOutGrace = () => vi.advanceTimersByTime(60_000);

describe("SpectrumHub", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("subscribes once for many watchers of one lane and stops on the last one", () => {
    const { fake, hub } = bench();

    const seen: number[] = [];
    const first = hub.subscribe("s1", "iq", () => seen.push(1));
    const second = hub.subscribe("s1", "iq", () => seen.push(2));
    expect(subscribes(fake.sent)).toHaveLength(1);

    fake.started(9, "s1", "iq");
    fake.push(9);
    expect(seen).toEqual([1, 2]);

    first();
    expect(unsubscribes(fake.sent)).toHaveLength(0);
    fake.push(9);
    expect(seen).toEqual([1, 2, 2]);

    second();
    waitOutGrace();
    expect(unsubscribes(fake.sent)).toHaveLength(1);
    expect(hub.watched()).toEqual([]);
  });

  it("keeps the stream through a face that lets go and comes straight back", () => {
    const { fake, hub } = bench();

    hub.subscribe("s1", "iq", () => {})();
    expect(hub.watched()).toEqual([{ node: "s1", port: "iq" }]);
    expect(unsubscribes(fake.sent)).toHaveLength(0);

    const seen: number[] = [];
    hub.subscribe("s1", "iq", (frame) => seen.push(frame.streamId));
    waitOutGrace();
    expect(unsubscribes(fake.sent)).toHaveLength(0);
    expect(subscribes(fake.sent)).toHaveLength(1);

    fake.started(9, "s1", "iq");
    fake.push(9);
    expect(seen).toEqual([9]);
  });

  it("routes frames by the id the server allocated, not by the device set", () => {
    const { fake, hub } = bench();
    const one: number[] = [];
    const two: number[] = [];
    hub.subscribe("s1", "iq", (frame) => one.push(frame.streamId));
    hub.subscribe("s2", "iq", (frame) => two.push(frame.streamId));
    fake.started(41, "s1", "iq");
    fake.started(42, "s2", "iq");

    fake.push(41);
    fake.push(42);
    fake.push(7);
    fake.push(1);
    expect(one).toEqual([41]);
    expect(two).toEqual([42]);
    expect(subscribes(fake.sent)).toHaveLength(2);
  });

  it("keeps two lanes of one radio apart", () => {
    const { fake, hub } = bench();
    const lane0: number[] = [];
    const lane2: number[] = [];
    const drop0 = hub.subscribe("s1", "iq", (frame) => lane0.push(frame.streamId));
    hub.subscribe("s1", "iq3", (frame) => lane2.push(frame.streamId));
    expect(subscribes(fake.sent).map((c) => c.data.port)).toEqual(["iq", "iq3"]);

    fake.started(10, "s1", "iq");
    fake.started(11, "s1", "iq3");
    fake.push(10);
    fake.push(11);
    expect(lane0).toEqual([10]);
    expect(lane2).toEqual([11]);

    drop0();
    waitOutGrace();
    expect(unsubscribes(fake.sent)).toEqual([
      { type: "UnsubscribePort", data: { node: "s1", port: "iq" } },
    ]);
    fake.stopped(10);
    fake.push(10);
    fake.push(11);
    expect(lane0).toEqual([10]);
    expect(lane2).toEqual([11, 11]);
    expect(hub.watched()).toEqual([{ node: "s1", port: "iq3" }]);
  });

  it("re-subscribes every lane still watched when the socket comes back", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    hub.subscribe("s1", "iq4", () => {});
    hub.subscribe("s3", "iq", () => {});
    fake.sent.length = 0;

    fake.reconnect();
    expect(
      subscribes(fake.sent)
        .map((c) => `${c.data.node}:${c.data.port}`)
        .toSorted(),
    ).toEqual(["s1:iq", "s1:iq4", "s3:iq"]);
  });

  it("forgets the ids of a connection that dropped", () => {
    const { fake, hub } = bench();
    const seen: number[] = [];
    hub.subscribe("s1", "iq", (frame) => seen.push(frame.streamId));
    fake.started(5, "s1", "iq");
    fake.push(5);

    fake.reconnect();
    fake.push(5);
    expect(seen, "the stale id routed a frame after the reconnect").toEqual([5]);

    fake.started(6, "s1", "iq");
    fake.push(6);
    expect(seen).toEqual([5, 6]);
  });

  it("carries watchers onto a replacement socket and lets go of the old one", () => {
    const first = fakeSocket();
    const { hub, ports } = bench(first);
    hub.subscribe("s2", "iq2", () => {});

    const second = fakeSocket();
    ports.attach(second.socket);
    hub.attach(second.socket);
    expect(first.attached()).toBe(false);
    expect(subscribes(second.sent).map((c) => `${c.data.node}:${c.data.port}`)).toEqual(["s2:iq2"]);

    hub.detach();
    expect(second.attached()).toBe(false);
  });
});

function rowsOf(history: { rows: Uint8Array; count: number; bins: number }): number[][] {
  return Array.from({ length: history.count }, (_, row) => [
    ...history.rows.subarray(row * history.bins, (row + 1) * history.bins),
  ]);
}

describe("spectrum resolution", () => {
  it("keeps a narrow peak when rows lose resolution", () => {
    expect([...resampleRows(Uint8Array.from([0, 9, 0, 0]), 1, 4, 2)]).toEqual([9, 0]);
  });

  it("stretches rows that gain resolution", () => {
    expect([...resampleRows(Uint8Array.from([1, 2]), 1, 2, 4)]).toEqual([1, 1, 2, 2]);
  });
});

describe("SpectrumHub resolution", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("subscribes at the widest resolution any watcher wants and settles back", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    const dropWide = hub.subscribe("s1", "iq", () => {}, 4096);
    dropWide();

    const asked = subscribes(fake.sent).map((c) => c.data.params?.bins ?? 0);
    expect(asked).toEqual([1024, 4096, 1024]);
  });
});

describe("SpectrumHub history", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("is empty for a lane nobody has watched", () => {
    const { hub } = bench();
    expect(hub.history("s1", "iq")).toEqual({
      rows: new Uint8Array(0),
      count: 0,
      bins: 0,
      meta: [],
    });
    expect(hub.latest("s1", "iq")).toBeNull();
  });

  it("keeps the rows of a lane whose face has gone, oldest first", () => {
    const { fake, hub } = bench();
    const drop = hub.subscribe("s1", "iq", () => {});
    fake.started(9, "s1", "iq");
    fake.push(9, [1, 2]);
    fake.push(9, [3, 4]);

    drop();
    fake.push(9, [5, 6]);

    expect(rowsOf(hub.history("s1", "iq"))).toEqual([
      [1, 2],
      [3, 4],
      [5, 6],
    ]);
    expect(hub.latest("s1", "iq")?.centerHz).toBe(100e6);
  });

  it("keeps each row's own dB window, aligned with the rows", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    fake.started(9, "s1", "iq");
    fake.push(9, [1, 2], { dbMin: -110, dbMax: -20 });
    fake.push(9, [3, 4], { dbMin: -100, dbMax: 0 });

    const history = hub.history("s1", "iq");
    expect(history.meta).toHaveLength(2);
    expect(history.meta[0]).toMatchObject({ dbMin: -110, dbMax: -20, spanHz: 2e6 });
    expect(history.meta[1]).toMatchObject({ dbMin: -100, dbMax: 0, centerHz: 100e6 });
  });

  it("keeps the metadata through a resolution change", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    fake.started(9, "s1", "iq");
    fake.push(9, [1, 2]);
    fake.push(9, [7, 8, 9]);

    const history = hub.history("s1", "iq");
    expect(history.count).toBe(2);
    expect(history.meta).toHaveLength(2);
  });

  it("keeps the newest rows once the ring has wrapped", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    fake.started(9, "s1", "iq");
    for (let row = 0; row < SPECTRUM_HISTORY_ROWS + 2; row++) {
      fake.push(9, [row & 0xff, 0]);
    }

    const history = hub.history("s1", "iq");
    expect(history.count).toBe(SPECTRUM_HISTORY_ROWS);
    const rows = rowsOf(history);
    expect(rows[0]).toEqual([2, 0]);
    expect(rows[rows.length - 1]).toEqual([(SPECTRUM_HISTORY_ROWS + 1) & 0xff, 0]);
  });

  it("rescales what it held when the bin count changes", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    fake.started(9, "s1", "iq");
    fake.push(9, [1, 2]);
    fake.push(9, [7, 8, 9]);

    expect(rowsOf(hub.history("s1", "iq"))).toEqual([
      [1, 2, 2],
      [7, 8, 9],
    ]);
  });

  it("forgets a lane nobody came back for", () => {
    const { fake, hub } = bench();
    const drop = hub.subscribe("s1", "iq", () => {});
    fake.started(9, "s1", "iq");
    fake.push(9, [1, 2]);

    drop();
    waitOutGrace();
    expect(hub.history("s1", "iq").count).toBe(0);
    expect(hub.latest("s1", "iq")).toBeNull();
    expect(unsubscribes(fake.sent)).toHaveLength(1);
  });

  it("re-subscribes a lane still inside its grace when the socket comes back", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {})();
    fake.sent.length = 0;

    fake.reconnect();
    expect(subscribes(fake.sent)).toHaveLength(1);
  });

  it("slows every lane while the page is hidden and restores it when it shows", () => {
    const { fake, hub } = bench();
    hub.subscribe("s1", "iq", () => {});
    const fps = () => subscribes(fake.sent).map((c) => c.data.params?.fps ?? 0);

    hub.setVisible(false);
    fake.reconnect();
    hub.setVisible(true);
    expect(fps()).toEqual([SPECTRUM_FPS, SPECTRUM_HIDDEN_FPS, SPECTRUM_HIDDEN_FPS, SPECTRUM_FPS]);
    expect(unsubscribes(fake.sent)).toHaveLength(0);
  });
});
