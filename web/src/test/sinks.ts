import type {
  DecodedRecord,
  DecoderEvent,
  Entity,
  EventKind,
  Hue,
  Presentation,
} from "../lib/types";

export function sinkRecord(
  source: string,
  event: DecoderEvent,
  view: Presentation,
  at = "2026-09-21T10:00:00Z",
): DecodedRecord {
  return {
    origin: { node: source, transmission: 0 },
    device_set: 1,
    channel: 1,
    at,
    freq_hz: 145_000_000,
    event,
    view,
  };
}

export function sinkEntity(
  source: string,
  kind: EventKind,
  id: string,
  event: DecoderEvent,
  view: Presentation,
  hue: Hue = "teal",
): Entity {
  return {
    key: { source, kind, id },
    hue,
    event,
    view,
    freq_hz: 1_090_000_000,
    first_seen: "2026-09-21T10:00:00Z",
    last_seen: "2026-09-21T10:00:00Z",
    frames: 1,
  };
}
