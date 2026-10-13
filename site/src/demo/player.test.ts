import { describe, expect, it } from "vitest";
import type { DemoSession } from "../../../web/e2e/demoSession";
import { Player } from "./player";

const SPECTRUM = {
  type: "SubscribePort",
  data: { node: "scope", port: "iq", params: { fps: 30, bins: 1024 } },
};
const STARTED = JSON.stringify({
  type: "PortStream",
  data: { stream_id: 4, node: "scope", port: "iq", source: "dev", kind: "spectrum" },
});

function session(): DemoSession {
  const bytes = new Uint8Array(16);
  bytes[0] = 1;
  return {
    scene: "test",
    title: "test",
    speaker: null,
    loopMs: 1000,
    responses: [],
    greeting: ['{"type":"Hello"}'],
    streams: [
      {
        key: 'Port:[["node","scope"],["port","iq"]]',
        started: STARTED,
        frames: [{ at: 100, binary: btoa(String.fromCharCode(...bytes)) }],
      },
    ],
    events: [{ at: 500, text: '{"type":"ChannelLevels"}' }],
  };
}

function harness() {
  let clock = 0;
  const sent: (string | ArrayBuffer)[] = [];
  const player = new Player(
    session(),
    (data) => sent.push(data),
    () => clock,
  );
  return {
    player,
    sent,
    advance(ms: number) {
      clock += ms;
      player.tick();
    },
  };
}

describe("Player", () => {
  it("greets on open", () => {
    const { player, sent } = harness();
    player.open();
    expect(sent).toEqual(['{"type":"Hello"}']);
  });

  it("answers a recorded subscription with its stream", () => {
    const { player, sent, advance } = harness();
    player.open();
    player.command(JSON.stringify(SPECTRUM));
    advance(200);
    expect(sent[1]).toBe(STARTED);
    expect(sent[2]).toBeInstanceOf(ArrayBuffer);
  });

  it("sends no frames without a subscription", () => {
    const { player, sent, advance } = harness();
    player.open();
    advance(200);
    expect(sent.some((data) => data instanceof ArrayBuffer)).toBe(false);
  });

  it("stops frames after an unsubscribe", () => {
    const { player, sent, advance } = harness();
    player.open();
    player.command(JSON.stringify(SPECTRUM));
    player.command(
      JSON.stringify({ type: "UnsubscribePort", data: { node: "scope", port: "iq" } }),
    );
    advance(200);
    expect(sent.some((data) => data instanceof ArrayBuffer)).toBe(false);
  });

  it("loops recorded events", () => {
    const { player, sent, advance } = harness();
    player.open();
    advance(2600);
    expect(sent.filter((data) => data === '{"type":"ChannelLevels"}')).toHaveLength(3);
  });
});
