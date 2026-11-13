import { describe, expect, it, vi } from "vitest";
import { paramsWith } from "../components/channelSettings";
import { createChannelQueue } from "./channelQueue";
import type { ChannelSettings } from "./types";

function tuned(hz: number, level = -70): ChannelSettings {
  return {
    frequency_hz: hz,
    squelch: { mode: "manual", level_db: level },
    params: paramsWith("probe", {}),
  };
}

interface Sent {
  node: string;
  settings: ChannelSettings;
  base: ChannelSettings;
  ack: () => Promise<void>;
  refuse: () => Promise<void>;
}

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i += 1) {
    await Promise.resolve();
  }
}

function rig(start: ChannelSettings) {
  const sent: Sent[] = [];
  const shown: ChannelSettings[] = [];
  const failed = vi.fn();
  const drained = vi.fn();
  const push = createChannelQueue({
    read: () => start,
    show: (_node, settings) => shown.push(settings),
    send: (node, settings, base) =>
      new Promise<void>((resolve, reject) => {
        sent.push({
          node,
          settings,
          base,
          ack: () => {
            resolve();
            return flush();
          },
          refuse: () => {
            reject(new Error("refused"));
            return flush();
          },
        });
      }),
    failed,
    drained,
  });
  const edit = (node: string, change: Partial<ChannelSettings>): void =>
    push(node, (current) => ({ ...current, ...change }));
  return { sent, shown, failed, drained, edit };
}

describe("createChannelQueue", () => {
  it("sends the first edit straight away against the cached settings", () => {
    const { sent, shown, edit } = rig(tuned(100));

    edit("a", { frequency_hz: 101 });

    expect(sent.map((s) => [s.settings, s.base])).toEqual([[tuned(101), tuned(100)]]);
    expect(shown).toEqual([tuned(101)]);
  });

  it("keeps one PATCH in flight per channel and coalesces the rest", async () => {
    const { sent, edit } = rig(tuned(100));

    edit("a", { frequency_hz: 101 });
    edit("a", { frequency_hz: 102 });
    edit("a", { squelch: { mode: "manual", level_db: -60 } });
    expect(sent).toHaveLength(1);

    await sent[0]?.ack();

    expect(sent).toHaveLength(2);
    expect(sent[1]?.settings).toEqual(tuned(102, -60));
    expect(sent[1]?.base).toEqual(tuned(101));
  });

  it("builds each edit on the last local edit, not a stale cache", async () => {
    const { sent, edit } = rig(tuned(100));

    edit("a", { frequency_hz: 101 });
    edit("a", { squelch: { mode: "manual", level_db: -50 } });
    await sent[0]?.ack();

    expect(sent[1]?.settings.frequency_hz).toBe(101);
  });

  it("lets the last edit win when an earlier PATCH is refused", async () => {
    const { sent, failed, drained, edit } = rig(tuned(100));

    edit("a", { frequency_hz: 101 });
    edit("a", { frequency_hz: 102 });
    await sent[0]?.refuse();

    expect(failed).toHaveBeenCalledOnce();
    expect(sent[1]?.settings).toEqual(tuned(102));
    expect(sent[1]?.base).toEqual(tuned(100));
    await sent[1]?.ack();
    expect(drained).toHaveBeenCalledOnce();
  });

  it("drains and refetches once nothing is left to send", async () => {
    const { sent, drained, edit } = rig(tuned(100));

    edit("a", { frequency_hz: 101 });
    expect(drained).not.toHaveBeenCalled();
    await sent[0]?.ack();

    expect(drained).toHaveBeenCalledOnce();
    edit("a", { frequency_hz: 103 });
    expect(sent[1]?.base).toEqual(tuned(100));
  });

  it("runs channels independently", () => {
    const { sent, edit } = rig(tuned(100));

    edit("a", { frequency_hz: 101 });
    edit("b", { frequency_hz: 201 });

    expect(sent.map((s) => s.node)).toEqual(["a", "b"]);
  });
});
