import { create } from "zustand";
import type { ServerEvent } from "./types";

type Health = Extract<ServerEvent, { type: "PipelineHealth" }>["data"];
export const usePipelineHealth = create<{
  health: Health | null;
  observe: (event: ServerEvent) => void;
  reset: () => void;
}>((set) => ({
  health: null,
  observe: (event) => {
    if (event.type === "PipelineHealth") set({ health: event.data });
  },
  reset: () => set({ health: null }),
}));

type Queue = Health["queues"][number];

export interface QueueSummary {
  oldestMs: number;
  dropped: number;
  detail: string;
}

export const QUEUE_LAG_MS = 100;

export function queueLagging(summary: QueueSummary | null): summary is QueueSummary {
  return summary !== null && summary.oldestMs >= QUEUE_LAG_MS;
}

export function queueSummary(health: Health | null, deviceSet: number): QueueSummary | null {
  return summarize(health?.queues.filter((queue) => queue.device_set === deviceSet) ?? []);
}

export function channelQueueSummary(
  health: Health | null,
  deviceSet: number,
  channel: number,
): QueueSummary | null {
  return summarize(
    health?.queues.filter((queue) => queue.device_set === deviceSet && queue.channel === channel) ??
      [],
  );
}

function summarize(queues: readonly Queue[]): QueueSummary | null {
  if (queues.length === 0) return null;
  return {
    oldestMs: Math.max(...queues.map((queue) => queue.health.oldest_ms)),
    dropped: queues.reduce((sum, queue) => sum + queue.health.dropped, 0),
    detail: queues
      .map(
        (queue) =>
          `${queue.stage} ${queue.stream}${queue.channel === null ? "" : `/${queue.channel}`}: ${queue.health.queued}/${queue.health.capacity}, ${queue.health.oldest_ms.toFixed(1)} ms, ${queue.health.dropped} dropped`,
      )
      .join("\n"),
  };
}

export const LINK_SLOW_MS = 250;
export const LINK_LOSSY = 0.02;

export interface LinkTrouble {
  label: string;
  detail: string;
}

export function linkTrouble(health: Health | null): LinkTrouble | null {
  if (health === null) return null;
  const { link, websocket } = health;
  const found: string[] = [];
  if (link != null && link.loss >= LINK_LOSSY) found.push(`${(link.loss * 100).toFixed(0)}% loss`);
  if (link != null && link.rtt_ms >= LINK_SLOW_MS) found.push(`${link.rtt_ms.toFixed(0)} ms`);
  if (websocket.oldest_ms >= QUEUE_LAG_MS)
    found.push(`${websocket.oldest_ms.toFixed(0)} ms behind`);
  const label = found[0];
  if (label === undefined) return null;
  const path =
    link == null
      ? []
      : [
          `Round trip ${link.rtt_ms.toFixed(0)} ms`,
          `${(link.loss * 100).toFixed(1)}% of packets lost`,
        ];
  return {
    label,
    detail: [
      ...path,
      `Sending ${websocket.oldest_ms.toFixed(0)} ms behind`,
      `${websocket.dropped} frames dropped`,
    ].join("\n"),
  };
}
