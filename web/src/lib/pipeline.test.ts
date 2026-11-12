import { afterEach, expect, it } from "vitest";
import {
  channelQueueSummary,
  linkTrouble,
  QUEUE_LAG_MS,
  queueLagging,
  queueSummary,
  usePipelineHealth,
} from "./pipeline";

afterEach(() => usePipelineHealth.getState().reset());

function queue(channel: number | null, oldest_ms: number, dropped: number) {
  return {
    device_set: 1,
    stream: 0,
    channel,
    stage: "spectrum" as const,
    health: { queued: 1, capacity: 16, oldest_ms, dropped },
  };
}

it("publishes stage metrics and clears stale metrics on disconnect", () => {
  usePipelineHealth.getState().observe({
    type: "PipelineHealth",
    data: {
      queues: [
        {
          device_set: 1,
          stream: 0,
          channel: null,
          stage: "capture",
          health: { queued: 2400, capacity: 240000, oldest_ms: 1, dropped: 5 },
        },
      ],
      websocket: { queued: 1024, capacity: 16777216, oldest_ms: 2, dropped: 3 },
    },
  });
  expect(usePipelineHealth.getState().health?.queues[0]?.health.dropped).toBe(5);
  usePipelineHealth.getState().reset();
  expect(usePipelineHealth.getState().health).toBeNull();
});

it("summarizes the queues of one device set and leaves the rest alone", () => {
  const health = {
    queues: [
      {
        device_set: 1,
        stream: 0,
        channel: null,
        stage: "capture" as const,
        health: { queued: 2400, capacity: 240000, oldest_ms: 1.25, dropped: 5 },
      },
      {
        device_set: 1,
        stream: 0,
        channel: 3,
        stage: "spectrum" as const,
        health: { queued: 8, capacity: 16, oldest_ms: 4.5, dropped: 0 },
      },
      {
        device_set: 2,
        stream: 0,
        channel: null,
        stage: "capture" as const,
        health: { queued: 1, capacity: 16, oldest_ms: 99, dropped: 0 },
      },
    ],
    websocket: { queued: 0, capacity: 16777216, oldest_ms: 0, dropped: 0 },
  };
  const summary = queueSummary(health, 1);
  expect(summary?.oldestMs).toBe(4.5);
  expect(summary?.dropped).toBe(5);
  expect(summary?.detail).toBe(
    "capture 0: 2400/240000, 1.3 ms, 5 dropped\nspectrum 0/3: 8/16, 4.5 ms, 0 dropped",
  );
  expect(queueSummary(health, 7)).toBeNull();
  expect(queueSummary(null, 1)).toBeNull();
});

it("summarizes only the queues of one decoder", () => {
  const health = {
    queues: [queue(null, 9, 1), queue(3, 2, 4), queue(3, 6, 1), queue(4, 50, 7)],
    websocket: { queued: 0, capacity: 16, oldest_ms: 0, dropped: 0 },
  };
  const summary = channelQueueSummary(health, 1, 3);
  expect(summary?.oldestMs).toBe(6);
  expect(summary?.dropped).toBe(5);
  expect(channelQueueSummary(health, 1, 9)).toBeNull();
  expect(channelQueueSummary(health, 2, 3)).toBeNull();
});

function lagOf(oldestMs: number) {
  return { oldestMs, dropped: 0, detail: "" };
}

it("calls a queue lagging only once it holds a noticeable delay", () => {
  expect(queueLagging(null)).toBe(false);
  expect(queueLagging(lagOf(0))).toBe(false);
  expect(queueLagging(lagOf(QUEUE_LAG_MS - 1))).toBe(false);
  expect(queueLagging(lagOf(QUEUE_LAG_MS))).toBe(true);
});

function linked(
  link: { rtt_ms: number; loss: number } | null,
  oldest_ms = 0,
): Parameters<typeof linkTrouble>[0] {
  return {
    queues: [],
    websocket: { queued: 0, capacity: 16777216, oldest_ms, dropped: 7 },
    link,
  };
}

it("says nothing about a link that is fine or not measured", () => {
  expect(linkTrouble(null)).toBeNull();
  expect(linkTrouble(linked(null))).toBeNull();
  expect(linkTrouble(linked({ rtt_ms: 40, loss: 0.001 }))).toBeNull();
});

it("names the worst of loss, round trip and a sender that falls behind", () => {
  expect(linkTrouble(linked({ rtt_ms: 400, loss: 0.125 }))).toEqual({
    label: "13% loss",
    detail: "Round trip 400 ms\n12.5% of packets lost\nSending 0 ms behind\n7 frames dropped",
  });
  expect(linkTrouble(linked({ rtt_ms: 400, loss: 0 }))?.label).toBe("400 ms");
  expect(linkTrouble(linked(null, QUEUE_LAG_MS))).toEqual({
    label: "100 ms behind",
    detail: "Sending 100 ms behind\n7 frames dropped",
  });
});
