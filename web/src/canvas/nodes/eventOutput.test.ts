import { describe, expect, it } from "vitest";
import type { FeedEntry } from "../../lib/types";
import { eventOutputConfigured, feedTarget, newOutputTarget, outputServices } from "./eventOutput";

const PROBE_FEED: FeedEntry = {
  format: "probe",
  name: "Probe TCP",
  default_address: "127.0.0.1:30999",
  kinds: [],
};

const matrix = (access_token: string) =>
  eventOutputConfigured({
    service: "matrix",
    homeserver_url: "https://matrix.example",
    room_id: "!radio:matrix.example",
    access_token,
  });

const mqtt = (broker_url: string, topic: string) =>
  eventOutputConfigured({ service: "mqtt", broker_url, topic, username: "", password: "" });

const postgres = (url: string, table: string, username: string) =>
  eventOutputConfigured({ service: "postgres", url, table, username, password: "" });

const influx = (url: string, bucket: string) =>
  eventOutputConfigured({ service: "influx", url, bucket, org: "", token: "" });

const has = (feeds: FeedEntry[]) =>
  outputServices(false, "mqtt", feeds).some((option) => option.value === "feed");

const offered = (notify: boolean, current: "desktop" | "mqtt") =>
  outputServices(notify, current).some((option) => option.value === "desktop");

describe("event output configuration", () => {
  it("saves recordings with nothing to configure", () => {
    expect(newOutputTarget("recordings")).toEqual({ service: "recordings" });
    expect(eventOutputConfigured({ service: "recordings" })).toBe(true);
  });

  it("offers notifications only in the desktop app", () => {
    expect(newOutputTarget("desktop")).toEqual({ service: "desktop" });
    expect(eventOutputConfigured({ service: "desktop" })).toBe(true);
    expect(offered(true, "mqtt")).toBe(true);
    expect(offered(false, "mqtt")).toBe(false);
    expect(offered(false, "desktop")).toBe(true);
  });

  it("appends CSV once a file name is set", () => {
    expect(newOutputTarget("csv")).toEqual({ service: "csv", file: "events" });
    expect(eventOutputConfigured({ service: "csv", file: "events" })).toBe(true);
    expect(eventOutputConfigured({ service: "csv", file: " " })).toBe(false);
  });

  it("opens a feed only after a format, an address and explicit enable", () => {
    const closed = { service: "feed", format: "probe", address: "127.0.0.1:30999" } as const;
    expect(eventOutputConfigured({ ...closed, enabled: false })).toBe(false);
    expect(eventOutputConfigured({ ...closed, address: "", enabled: true })).toBe(false);
    expect(eventOutputConfigured({ ...closed, format: "", enabled: true })).toBe(false);
    expect(eventOutputConfigured({ ...closed, enabled: true })).toBe(true);
  });

  it("starts a feed in the first format the catalog lists, on its own address", () => {
    expect(newOutputTarget("feed", [PROBE_FEED])).toEqual(feedTarget(PROBE_FEED));
    expect(feedTarget(PROBE_FEED)).toEqual({
      service: "feed",
      format: "probe",
      address: "127.0.0.1:30999",
      enabled: false,
    });
  });

  it("offers a feed only when a decoder registered one", () => {
    expect(has([])).toBe(false);
    expect(has([PROBE_FEED])).toBe(true);
    expect(outputServices(false, "feed").some((option) => option.value === "feed")).toBe(true);
  });

  it("needs a webhook endpoint", () => {
    expect(eventOutputConfigured({ service: "webhook", url: "", format: "json" })).toBe(false);
    expect(eventOutputConfigured({ service: "webhook", url: "   ", format: "json" })).toBe(false);
    expect(
      eventOutputConfigured({
        service: "webhook",
        url: "https://discord.com/api/webhooks/1/token",
        format: "discord",
      }),
    ).toBe(true);
  });

  it("needs all Matrix credentials", () => {
    expect(matrix("")).toBe(false);
    expect(matrix("   ")).toBe(false);
    expect(matrix("secret")).toBe(true);
  });

  it("needs an MQTT broker and a topic, but no credentials", () => {
    expect(mqtt("", "sdrmm/events")).toBe(false);
    expect(mqtt("mqtts://broker.example", "  ")).toBe(false);
    expect(mqtt("mqtts://broker.example", "sdrmm/events")).toBe(true);
  });

  it("needs a Postgres server, table and user", () => {
    expect(postgres("postgres://db.example/radio", "sdrmm_events", "radio")).toBe(true);
    expect(postgres("", "sdrmm_events", "radio")).toBe(false);
    expect(postgres("postgres://db.example/radio", " ", "radio")).toBe(false);
    expect(postgres("postgres://db.example/radio", "sdrmm_events", "")).toBe(false);
  });

  it("needs an InfluxDB server and bucket, but no token", () => {
    expect(influx("http://127.0.0.1:8086", "radio")).toBe(true);
    expect(influx("", "radio")).toBe(false);
    expect(influx("http://127.0.0.1:8086", "")).toBe(false);
  });

  it("starts every service unconfigured", () => {
    for (const service of ["webhook", "matrix", "mqtt", "feed", "postgres", "influx"] as const) {
      const target = newOutputTarget(service, [PROBE_FEED]);
      expect(target.service).toBe(service);
      expect(eventOutputConfigured(target)).toBe(false);
    }
  });
});
