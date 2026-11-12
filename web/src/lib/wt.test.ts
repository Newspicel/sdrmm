import { afterEach, describe, expect, it } from "vitest";
import { unhex } from "./hex";
import { lengthPrefixed, MessageSplitter, webTransportUrl, webTransportUsable } from "./wt";

function texts(messages: Uint8Array[]): string[] {
  return messages.map((message) => new TextDecoder().decode(message));
}

function joined(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

function fakeLocation(protocol: string, hostname: string): void {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { location: { protocol, hostname } },
  });
}

describe("MessageSplitter", () => {
  it("splits several messages that arrive in one chunk", () => {
    const splitter = new MessageSplitter();
    const chunk = joined(lengthPrefixed("one"), lengthPrefixed(""), lengthPrefixed("three"));
    expect(texts(splitter.push(chunk))).toEqual(["one", "", "three"]);
  });

  it("waits for a message cut anywhere, even inside its length", () => {
    const whole = lengthPrefixed("across chunks");
    for (let cut = 1; cut < whole.length; cut++) {
      const splitter = new MessageSplitter();
      expect(splitter.push(whole.slice(0, cut))).toEqual([]);
      expect(texts(splitter.push(whole.slice(cut)))).toEqual(["across chunks"]);
    }
  });

  it("refuses a length no frame can have", () => {
    const splitter = new MessageSplitter();
    expect(() => splitter.push(Uint8Array.of(0xff, 0xff, 0xff, 0xff))).toThrow(/over the limit/);
  });
});

describe("the session address", () => {
  afterEach(() => {
    Reflect.deleteProperty(globalThis, "window");
    Reflect.deleteProperty(globalThis, "WebTransport");
  });

  it("is this host on the advertised port, with the token in the query", () => {
    fakeLocation("https:", "radio.local");
    const info = { port: 8443, certificate_sha256: "00" };
    expect(webTransportUrl(info, null)).toBe("https://radio.local:8443/api/wt");
    expect(webTransportUrl(info, "s3cret/ä")).toBe(
      "https://radio.local:8443/api/wt?token=s3cret%2F%C3%A4",
    );
  });

  it("is only tried from an HTTPS page in a browser that has WebTransport", () => {
    fakeLocation("https:", "radio.local");
    expect(webTransportUsable()).toBe(false);
    Object.defineProperty(globalThis, "WebTransport", { configurable: true, value: Object });
    expect(webTransportUsable()).toBe(true);
    fakeLocation("http:", "localhost");
    expect(webTransportUsable()).toBe(false);
  });
});

describe("unhex", () => {
  it("reads a certificate hash and refuses anything else", () => {
    expect(unhex("00ff7a")).toEqual(Uint8Array.of(0, 255, 122));
    expect(unhex("0")).toBeNull();
    expect(unhex("zz")).toBeNull();
  });
});
