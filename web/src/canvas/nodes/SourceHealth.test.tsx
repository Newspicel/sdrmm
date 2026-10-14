import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, it } from "vitest";
import { QUEUE_LAG_MS, usePipelineHealth } from "../../lib/pipeline";
import type { DeviceSet } from "../../lib/types";
import { SourceHealth } from "./SourceHealth";

afterEach(() => usePipelineHealth.getState().reset());

const set: DeviceSet = {
  id: 1,
  device: { driver: "virtual", key: "v", label: "v" },
  capabilities: {
    freq_ranges: [],
    sample_rates: [],
    gains: [],
    antennas: [],
    bandwidths: [],
    extra: [],
    duplex: "rx_only",
    rx_streams: 1,
    coherence: "none",
    noise_source: "none",
  },
  settings: {},
  status: "running",
  channels: [],
  overruns: 0,
};

function queued(oldest_ms: number): string {
  usePipelineHealth.getState().observe({
    type: "PipelineHealth",
    data: {
      queues: [
        {
          device_set: 1,
          stream: 0,
          channel: null,
          stage: "capture",
          health: { queued: 1, capacity: 16, oldest_ms, dropped: 0 },
        },
      ],
      websocket: { queued: 0, capacity: 16, oldest_ms: 0, dropped: 0 },
    },
  });
  return renderToStaticMarkup(<SourceHealth set={set} />);
}

it("hides a healthy queue", () => {
  expect(queued(QUEUE_LAG_MS - 1)).not.toContain("Queue");
});

it("shows a lagging queue", () => {
  expect(queued(QUEUE_LAG_MS)).toContain("Queue");
});
