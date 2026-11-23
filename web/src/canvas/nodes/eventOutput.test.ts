import { describe, expect, it } from "vitest";
import type { FeedEntry } from "../../lib/types";
import {
  eventOutputConfigured,
  feedTarget,
  insertAt,
  messageShown,
  newOutputTarget,
  outputServices,
  signsIn,
  takesMessage,
} from "./eventOutput";

const PROBE_FEED: FeedEntry = {
  format: "probe",
  name: "Probe TCP",
  default_address: "127.0.0.1:30999",
  kinds: [],
};

const matrix = (account: string, room_id: string) =>
  eventOutputConfigured({ service: "matrix", account, room_id });

const mqtt = (account: string, topic: string) =>
  eventOutputConfigured({ service: "mqtt", account, topic });

const postgres = (account: string, table: string) =>
  eventOutputConfigured({ service: "postgres", account, table });

const influx = (account: string, bucket: string) =>
  eventOutputConfigured({ service: "influx", account, bucket });

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

  it("appends text once a file name is set", () => {
    expect(newOutputTarget("text")).toEqual({ service: "text", file: "events" });
    expect(eventOutputConfigured({ service: "text", file: " " })).toBe(false);
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

  it("needs a webhook account", () => {
    expect(eventOutputConfigured({ service: "webhook", account: "", format: "json" })).toBe(false);
    expect(eventOutputConfigured({ service: "webhook", account: " ", format: "json" })).toBe(false);
    expect(eventOutputConfigured({ service: "webhook", format: "json" })).toBe(false);
    expect(eventOutputConfigured({ service: "webhook", account: "a1", format: "discord" })).toBe(
      true,
    );
  });

  it("needs an account and a place for every other service that signs in", () => {
    for (const configured of [matrix, mqtt, postgres, influx]) {
      expect(configured("a1", "radio")).toBe(true);
      expect(configured("", "radio")).toBe(false);
      expect(configured("a1", " ")).toBe(false);
    }
  });

  it("knows which services sign in", () => {
    expect(signsIn(newOutputTarget("mqtt"))).toBe(true);
    expect(signsIn(newOutputTarget("csv"))).toBe(false);
    expect(newOutputTarget("postgres")).toEqual({
      service: "postgres",
      account: "",
      table: "sdrmm_events",
    });
  });

  it("starts every service unconfigured", () => {
    for (const service of ["webhook", "matrix", "mqtt", "feed", "postgres", "influx"] as const) {
      const target = newOutputTarget(service, [PROBE_FEED]);
      expect(target.service).toBe(service);
      expect(eventOutputConfigured(target)).toBe(false);
    }
  });
});

describe("message layouts", () => {
  it("go only on services a person reads", () => {
    expect(takesMessage("webhook")).toBe(true);
    expect(takesMessage("desktop")).toBe(true);
    expect(takesMessage("csv")).toBe(false);
    expect(takesMessage("postgres")).toBe(false);
  });

  it("show their first line, or usual when empty", () => {
    expect(messageShown("")).toBe("usual");
    expect(messageShown(" {headline}\n{freq}")).toBe("{headline}");
  });

  it("put a placeholder over the selection", () => {
    expect(insertAt("on  MHz", 3, 3, "{freq}")).toEqual({ text: "on {freq} MHz", caret: 9 });
    expect(insertAt("x", 0, 1, "{record.}")).toEqual({ text: "{record.}", caret: 8 });
  });
});
