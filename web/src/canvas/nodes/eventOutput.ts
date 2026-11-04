import type { EventOutputTarget, FeedEntry } from "../../lib/types";

export const OUTPUT_SERVICES = [
  { value: "recordings", label: "Recordings" },
  { value: "desktop", label: "Notification" },
  { value: "csv", label: "CSV file" },
  { value: "feed", label: "Feed" },
  { value: "webhook", label: "Webhook" },
  { value: "matrix", label: "Matrix" },
  { value: "mqtt", label: "MQTT" },
  { value: "postgres", label: "PostgreSQL" },
  { value: "influx", label: "InfluxDB" },
  { value: "tunnel", label: "Network interface" },
] as const;

export function outputServices(
  notify: boolean,
  current: EventOutputTarget["service"],
  feeds: readonly FeedEntry[] = [],
) {
  return OUTPUT_SERVICES.filter(
    (option) =>
      option.value === current ||
      ((option.value !== "desktop" || notify) && (option.value !== "feed" || feeds.length > 0)),
  );
}

type FeedTarget = Extract<EventOutputTarget, { service: "feed" }>;

/** A closed feed server in `feed`'s format on its usual address. */
export function feedTarget(feed: FeedEntry | undefined): FeedTarget {
  return {
    service: "feed",
    format: feed?.format ?? "",
    address: feed?.default_address ?? "",
    enabled: false,
  };
}

export const WEBHOOK_FORMATS = [
  { value: "json", label: "JSON" },
  { value: "discord", label: "Discord" },
] as const;

export function newOutputTarget(
  service: EventOutputTarget["service"],
  feeds: readonly FeedEntry[] = [],
): EventOutputTarget {
  switch (service) {
    case "recordings":
    case "desktop":
      return { service };
    case "csv":
      return { service, file: "events" };
    case "feed":
      return feedTarget(feeds[0]);
    case "tunnel":
      return { service, interface: "", address: "10.23.0.1", prefix: 24 };
    case "webhook":
      return { service, url: "", format: "json" };
    case "matrix":
      return { service, homeserver_url: "", room_id: "", access_token: "" };
    case "mqtt":
      return { service, broker_url: "", topic: "", username: "", password: "" };
    case "postgres":
      return { service, url: "", table: "sdrmm_events", username: "", password: "" };
    case "influx":
      return { service, url: "", bucket: "", org: "", token: "" };
  }
}

export function eventOutputConfigured(target: EventOutputTarget): boolean {
  switch (target.service) {
    case "recordings":
    case "desktop":
      return true;
    case "csv":
      return target.file.trim() !== "";
    case "feed":
      return (
        target.enabled === true && target.address.trim() !== "" && (target.format ?? "") !== ""
      );
    case "tunnel":
      return target.interface.trim() !== "";
    case "webhook":
      return target.url.trim() !== "";
    case "matrix":
      return (
        target.homeserver_url.trim() !== "" &&
        target.room_id.trim() !== "" &&
        target.access_token.trim() !== ""
      );
    case "mqtt":
      return target.broker_url.trim() !== "" && target.topic.trim() !== "";
    case "postgres":
      return [target.url, target.table, target.username ?? ""].every(
        (value) => value.trim() !== "",
      );
    case "influx":
      return target.url.trim() !== "" && target.bucket.trim() !== "";
  }
}
