import type { AccountKind } from "../../lib/accounts";
import type { EventOutputTarget, FeedEntry } from "../../lib/types";

export const OUTPUT_SERVICES = [
  { value: "recordings", label: "Recordings" },
  { value: "desktop", label: "Notification" },
  { value: "csv", label: "CSV file" },
  { value: "text", label: "Text file" },
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

/** The name a service goes by, as the settings list it. */
export function serviceLabel(
  service: EventOutputTarget["service"],
  feeds: readonly FeedEntry[] = [],
): string {
  return (
    outputServices(true, service, feeds).find((option) => option.value === service)?.label ??
    service
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

/** Services that send a message a person reads, so a layout can shape it. */
export function takesMessage(service: EventOutputTarget["service"]): boolean {
  return (
    service === "desktop" ||
    service === "text" ||
    service === "webhook" ||
    service === "matrix" ||
    service === "mqtt"
  );
}

export function placeholderToken(name: string): string {
  return `{${name}}`;
}

/** `text` with `token` put over the selection, and where the caret lands after it. */
export function insertAt(
  text: string,
  start: number,
  end: number,
  token: string,
): { text: string; caret: number } {
  const caret = token.endsWith(".}") ? start + token.length - 1 : start + token.length;
  return { text: text.slice(0, start) + token + text.slice(end), caret };
}

/** The layout's first line, as its chip shows it. */
export function messageShown(layout: string): string {
  const first = layout.trim().split("\n")[0] ?? "";
  return first === "" ? "usual" : first;
}

export function newOutputTarget(
  service: EventOutputTarget["service"],
  feeds: readonly FeedEntry[] = [],
): EventOutputTarget {
  switch (service) {
    case "recordings":
    case "desktop":
      return { service };
    case "csv":
    case "text":
      return { service, file: "events" };
    case "feed":
      return feedTarget(feeds[0]);
    case "tunnel":
      return { service, interface: "", address: "10.23.0.1", prefix: 24 };
    case "webhook":
      return { service, account: "", format: "json" };
    case "matrix":
      return { service, account: "", room_id: "" };
    case "mqtt":
      return { service, account: "", topic: "" };
    case "postgres":
      return { service, account: "", table: "sdrmm_events" };
    case "influx":
      return { service, account: "", bucket: "" };
  }
}

export type SigningTarget = Extract<EventOutputTarget, { service: AccountKind }>;

const ACCOUNT_SERVICES: ReadonlySet<EventOutputTarget["service"]> = new Set([
  "webhook",
  "matrix",
  "mqtt",
  "postgres",
  "influx",
]);

/** Whether the output signs in through an account of its own service. */
export function signsIn(target: EventOutputTarget): target is SigningTarget {
  return ACCOUNT_SERVICES.has(target.service);
}

export interface OutputPlace {
  label: string;
  title: string;
  value: string;
  set: (value: string) => EventOutputTarget;
}

/** Where inside its account's service an output sends. A webhook's account is the place. */
export function placeOf(target: SigningTarget): OutputPlace | undefined {
  switch (target.service) {
    case "webhook":
      return undefined;
    case "matrix":
      return {
        label: "Room",
        title: "Matrix room ID",
        value: target.room_id,
        set: (room_id) => ({ ...target, room_id }),
      };
    case "mqtt":
      return {
        label: "Topic",
        title: "MQTT topic",
        value: target.topic,
        set: (topic) => ({ ...target, topic }),
      };
    case "postgres":
      return {
        label: "Table",
        title: "Created on first write. Lowercase letters, digits and _",
        value: target.table,
        set: (table) => ({ ...target, table }),
      };
    case "influx":
      return {
        label: "Bucket",
        title: "Bucket, or database on InfluxDB 3",
        value: target.bucket,
        set: (bucket) => ({ ...target, bucket }),
      };
  }
}

export function eventOutputConfigured(target: EventOutputTarget): boolean {
  switch (target.service) {
    case "recordings":
    case "desktop":
      return true;
    case "csv":
    case "text":
      return target.file.trim() !== "";
    case "feed":
      return (
        target.enabled === true && target.address.trim() !== "" && (target.format ?? "") !== ""
      );
    case "tunnel":
      return target.interface.trim() !== "";
    case "webhook":
      return filled(target.account);
    case "matrix":
      return filled(target.account) && filled(target.room_id);
    case "mqtt":
      return filled(target.account) && filled(target.topic);
    case "postgres":
      return filled(target.account) && filled(target.table);
    case "influx":
      return filled(target.account) && filled(target.bucket);
  }
}

function filled(value: string | undefined): boolean {
  return (value ?? "").trim() !== "";
}
