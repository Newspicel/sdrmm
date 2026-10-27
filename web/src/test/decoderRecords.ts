import type { EntityOf } from "../lib/sinks";
import type { DecodedRecordOf, DecoderEventOf, DecoderKind } from "../lib/types";

export const NOW = Date.parse("2026-08-09T12:00:00Z");

export function record<K extends DecoderKind>(
  kind: K,
  data: DecoderEventOf<K>["data"],
  over: Partial<Omit<DecodedRecordOf<K>, "event">> = {},
): DecodedRecordOf<K> {
  return {
    at: "2026-08-09T12:00:00Z",
    device_set: 1,
    channel: 0,
    freq_hz: 1_090_000_000,
    event: { kind, data } as DecoderEventOf<K>,
    ...over,
  };
}

export function station<K extends DecoderKind>(
  kind: K,
  id: string,
  data: DecoderEventOf<K>["data"],
  over: Partial<EntityOf<K>> = {},
): EntityOf<K> {
  return {
    key: { source: "decoder", kind, id },
    hue: "teal",
    event: { kind, data } as DecoderEventOf<K>,
    view: { summary: id },
    freq_hz: 1_090_000_000,
    first_seen: new Date(NOW).toISOString(),
    last_seen: new Date(NOW).toISOString(),
    frames: 1,
    ...over,
  };
}
