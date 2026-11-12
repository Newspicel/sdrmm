import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtures from "../generated/frame-fixtures.json";
import { setToken } from "./auth";
import { clientEvents, resetEvents } from "./diagnostics";
import { LateAudio, SdrSocket, socketProtocols } from "./ws";
import { lengthPrefixed } from "./wt";

const RECONNECT_CEILING = 30_000;

const sockets: FakeWebSocket[] = [];

class FakeWebSocket {
  static readonly OPEN = 1;
  readyState = 0;
  binaryType = "";
  onopen: (() => void) | null = null;
  onclose: ((event: Pick<CloseEvent, "code" | "reason" | "wasClean">) => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;

  constructor(
    readonly url: string,
    readonly protocols: string[] = [],
  ) {
    sockets.push(this);
  }

  send(): void {}

  close(): void {
    this.readyState = 3;
  }

  accept(): void {
    this.readyState = 1;
    this.onopen?.();
  }

  drop(code = 1006, reason = ""): void {
    this.readyState = 3;
    this.onclose?.({ code, reason, wasClean: false });
  }
}

function fakeWindow(protocol = "http:") {
  const listeners = new Map<string, Set<() => void>>();
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      location: { protocol, host: "sdr.local", hostname: "sdr.local" },
      setTimeout: (fn: () => void, ms: number) => globalThis.setTimeout(fn, ms),
      clearTimeout: (id: number) => globalThis.clearTimeout(id),
      addEventListener: (type: string, fn: () => void) => {
        const existing = listeners.get(type) ?? new Set<() => void>();
        existing.add(fn);
        listeners.set(type, existing);
      },
      removeEventListener: (type: string, fn: () => void) => {
        listeners.get(type)?.delete(fn);
      },
    },
  });
  return {
    fire: (type: string) => {
      for (const fn of listeners.get(type) ?? []) {
        fn();
      }
    },
    count: (type: string) => listeners.get(type)?.size ?? 0,
  };
}

function latest(): FakeWebSocket {
  const socket = sockets.at(-1);
  if (socket === undefined) {
    throw new Error("no socket was opened");
  }
  return socket;
}

describe("SdrSocket reconnect", () => {
  let online: ReturnType<typeof fakeWindow>;

  beforeEach(() => {
    sockets.length = 0;
    vi.useFakeTimers();
    Object.defineProperty(globalThis, "WebSocket", {
      configurable: true,
      value: FakeWebSocket,
    });
    vi.spyOn(Math, "random").mockReturnValue(1);
    online = fakeWindow();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("doubles the delay while the server refuses the connection", () => {
    const socket = new SdrSocket();
    socket.connect();
    expect(sockets).toHaveLength(1);

    latest().drop();
    vi.advanceTimersByTime(999);
    expect(sockets).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(2);

    latest().drop();
    vi.advanceTimersByTime(1_999);
    expect(sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(3);
  });

  it("keeps backing off when a connection opens and dies straight away", () => {
    const socket = new SdrSocket();
    socket.connect();

    latest().accept();
    latest().drop();
    vi.advanceTimersByTime(1_000);
    expect(sockets).toHaveLength(2);

    latest().accept();
    latest().drop();
    vi.advanceTimersByTime(1_999);
    expect(sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(3);
  });

  it("resets the delay only after a connection that lasted", () => {
    const socket = new SdrSocket();
    socket.connect();

    latest().drop();
    vi.advanceTimersByTime(1_000);
    expect(sockets).toHaveLength(2);

    latest().accept();
    vi.advanceTimersByTime(10_000);
    latest().drop();

    vi.advanceTimersByTime(999);
    expect(sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(3);
  });

  it("never waits longer than the ceiling", () => {
    const socket = new SdrSocket();
    socket.connect();

    for (let i = 0; i < 10; i++) {
      latest().drop();
      vi.advanceTimersByTime(RECONNECT_CEILING);
    }
    const seen = sockets.length;

    latest().drop();
    vi.advanceTimersByTime(RECONNECT_CEILING - 1);
    expect(sockets).toHaveLength(seen);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(seen + 1);
  });

  it("spreads the retry across the second half of the window", () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const socket = new SdrSocket();
    socket.connect();

    latest().drop();
    vi.advanceTimersByTime(499);
    expect(sockets).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(2);
  });

  it("retries at once when the network comes back", () => {
    const socket = new SdrSocket();
    socket.connect();

    latest().drop();
    vi.advanceTimersByTime(1_000);
    latest().drop();
    const waiting = sockets.length;

    online.fire("online");
    expect(sockets).toHaveLength(waiting + 1);
  });

  it("goes back to the shortest delay after the network returns", () => {
    const socket = new SdrSocket();
    socket.connect();

    latest().drop();
    vi.advanceTimersByTime(1_000);
    latest().drop();
    vi.advanceTimersByTime(2_000);

    online.fire("online");
    latest().drop();
    vi.advanceTimersByTime(999);
    const waiting = sockets.length;
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(waiting + 1);
  });

  it("drops the network listener when the socket is closed", () => {
    const socket = new SdrSocket();
    socket.connect();
    expect(online.count("online")).toBe(1);

    socket.close();
    expect(online.count("online")).toBe(0);
  });

  it("stops reconnecting once closed", () => {
    const socket = new SdrSocket();
    socket.connect();
    const opened = sockets.length;

    socket.close();
    vi.advanceTimersByTime(60_000);
    expect(sockets).toHaveLength(opened);
  });

  it("records why a live connection closed", () => {
    resetEvents();
    const socket = new SdrSocket();
    socket.connect();

    latest().drop();
    expect(clientEvents()).toHaveLength(0);

    vi.advanceTimersByTime(1_000);
    latest().accept();
    vi.advanceTimersByTime(58_200);
    latest().drop(4502, "window overrun");
    expect(clientEvents().at(-1)).toMatchObject({
      level: "warn",
      source: "socket",
      message: "closed 4502 window overrun after 58.2 s, unclean",
    });
  });
});

describe("SdrSocket messages", () => {
  beforeEach(() => {
    sockets.length = 0;
    Object.defineProperty(globalThis, "WebSocket", {
      configurable: true,
      value: FakeWebSocket,
    });
    fakeWindow();
    resetEvents();
  });

  it("records unreadable events and unknown frames instead of dropping them quietly", () => {
    const socket = new SdrSocket();
    const events: unknown[] = [];
    socket.on("event", (event) => events.push(event));
    socket.connect();
    latest().onmessage?.({ data: "{not json" } as MessageEvent);
    const unknown = new Uint8Array(16);
    unknown[0] = 1;
    unknown[1] = 42;
    latest().onmessage?.({ data: unknown.buffer } as MessageEvent);
    socket.close();
    expect(events).toEqual([]);
    expect(clientEvents().map((event) => event.message)).toEqual([
      "unreadable event",
      "unknown frame 42",
    ]);
  });
});

describe("SdrSocket credentials", () => {
  beforeEach(() => {
    sockets.length = 0;
    Object.defineProperty(globalThis, "WebSocket", {
      configurable: true,
      value: FakeWebSocket,
    });
    fakeWindow();
  });

  afterEach(() => setToken(null));

  it("offers the token as a subprotocol and keeps it out of the URL", () => {
    setToken("s3cret/ä");
    const socket = new SdrSocket();
    socket.connect();
    socket.close();
    expect(latest().url).toBe("ws://sdr.local/api/ws");
    expect(latest().protocols).toEqual(["sdrmm", "sdrmm.bearer.7333637265742fc3a4"]);
  });

  it("offers only the plain subprotocol without a token", () => {
    const socket = new SdrSocket();
    socket.connect();
    socket.close();
    expect(latest().url).toBe("ws://sdr.local/api/ws");
    expect(latest().protocols).toEqual(["sdrmm"]);
    expect(socketProtocols(null)).toEqual(["sdrmm"]);
  });
});

const sessions: FakeWebTransport[] = [];

class FakeWebTransport {
  static refuse = false;
  readonly ready: Promise<void>;
  readonly closed: Promise<{ closeCode: number; reason: string }>;
  readonly datagrams: { readable: ReadableStream<Uint8Array> };
  readonly incomingUnidirectionalStreams: ReadableStream<ReadableStream<Uint8Array>>;
  readonly sent: Uint8Array[] = [];
  events!: ReadableStreamDefaultController<Uint8Array>;
  audio!: ReadableStreamDefaultController<Uint8Array>;
  lanes!: ReadableStreamDefaultController<ReadableStream<Uint8Array>>;
  private end!: (closed: { closeCode: number; reason: string }) => void;

  constructor(
    readonly url: string,
    readonly options: WebTransportOptions,
  ) {
    this.ready = FakeWebTransport.refuse ? Promise.reject(new Error("refused")) : Promise.resolve();
    this.closed = new Promise((resolve) => {
      this.end = resolve;
    });
    this.datagrams = {
      readable: new ReadableStream({ start: (controller) => void (this.audio = controller) }),
    };
    this.incomingUnidirectionalStreams = new ReadableStream({
      start: (controller) => void (this.lanes = controller),
    });
    sessions.push(this);
  }

  createBidirectionalStream(): Promise<unknown> {
    return Promise.resolve({
      readable: new ReadableStream<Uint8Array>({
        start: (controller) => void (this.events = controller),
      }),
      writable: new WritableStream<Uint8Array>({ write: (chunk) => void this.sent.push(chunk) }),
    });
  }

  lane(...messages: Uint8Array[]): void {
    this.lanes.enqueue(
      new ReadableStream({
        start: (controller) => {
          for (const message of messages) {
            controller.enqueue(message);
          }
        },
      }),
    );
  }

  close(): void {
    this.end({ closeCode: 0, reason: "" });
  }
}

function fixture(name: string): Uint8Array {
  const found = (fixtures as [string, number[]][]).find(([kind]) => kind === name);
  if (found === undefined) {
    throw new Error(`no ${name} fixture`);
  }
  return Uint8Array.from(found[1]);
}

function framed(bytes: Uint8Array): Uint8Array {
  const out = new Uint8Array(4 + bytes.length);
  new DataView(out.buffer).setUint32(0, bytes.length, true);
  out.set(bytes, 4);
  return out;
}

const OFFER = { port: 8443, certificate_sha256: "ab".repeat(32) };

async function connected(socket: SdrSocket): Promise<FakeWebTransport> {
  socket.connect();
  await vi.waitFor(() => expect(socket.transport()).toBe("WebTransport"));
  const session = sessions.at(-1);
  if (session === undefined) {
    throw new Error("no session was opened");
  }
  return session;
}

describe("SdrSocket over WebTransport", () => {
  beforeEach(() => {
    sockets.length = 0;
    sessions.length = 0;
    FakeWebTransport.refuse = false;
    Object.defineProperty(globalThis, "WebSocket", { configurable: true, value: FakeWebSocket });
    Object.defineProperty(globalThis, "WebTransport", {
      configurable: true,
      value: FakeWebTransport,
    });
    fakeWindow("https:");
    resetEvents();
  });

  afterEach(() => {
    Reflect.deleteProperty(globalThis, "WebTransport");
    setToken(null);
  });

  it("pins the advertised certificate and carries the token in the address", async () => {
    setToken("s3cret");
    const socket = new SdrSocket("/api/ws", async () => OFFER);
    const session = await connected(socket);
    expect(session.url).toBe("https://sdr.local:8443/api/wt?token=s3cret");
    expect(session.options.serverCertificateHashes).toEqual([
      { algorithm: "sha-256", value: new Uint8Array(32).fill(0xab) },
    ]);
    expect(sockets).toHaveLength(0);
    socket.close();
  });

  it("is connected before anyone hears of it, so a first command is sent", async () => {
    const socket = new SdrSocket("/api/ws", async () => OFFER);
    socket.on("status", (up) => {
      if (up) {
        socket.send({ type: "SubscribeDiagnostics", data: { enabled: true } });
      }
    });
    const session = await connected(socket);
    await vi.waitFor(() => expect(session.sent).toHaveLength(1));
    expect(session.sent[0]).toEqual(
      lengthPrefixed('{"type":"SubscribeDiagnostics","data":{"enabled":true}}'),
    );
    socket.close();
  });

  it("reads events from the control stream, frames from lanes and audio from datagrams", async () => {
    const socket = new SdrSocket("/api/ws", async () => OFFER);
    const heard: string[] = [];
    socket.on("event", (event) => heard.push(event.type));
    socket.on("spectrum", () => heard.push("spectrum"));
    socket.on("audio", () => heard.push("audio"));
    const session = await connected(socket);

    session.events.enqueue(lengthPrefixed('{"type":"Error","data":{"message":"x"}}'));
    await vi.waitFor(() => expect(heard).toEqual(["Error"]));
    session.lane(framed(fixture("spectrum")), framed(fixture("spectrum")));
    await vi.waitFor(() => expect(heard).toEqual(["Error", "spectrum", "spectrum"]));
    session.audio.enqueue(fixture("audio"));
    await vi.waitFor(() => expect(heard).toEqual(["Error", "spectrum", "spectrum", "audio"]));
    session.audio.enqueue(fixture("audio"));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(heard).toHaveLength(4);
    socket.close();
  });

  it("reports a lost session and dials again", async () => {
    const socket = new SdrSocket("/api/ws", async () => OFFER);
    const status: boolean[] = [];
    socket.on("status", (up) => status.push(up));
    const session = await connected(socket);
    session.close();
    await vi.waitFor(() => expect(status).toEqual([true, false]));
    expect(socket.isConnected()).toBe(false);
    socket.close();
  });

  it("falls back to the WebSocket and leaves WebTransport alone for a while", async () => {
    FakeWebTransport.refuse = true;
    const socket = new SdrSocket("/api/ws", async () => OFFER);
    socket.connect();
    await vi.waitFor(() => expect(sockets).toHaveLength(1));
    expect(latest().url).toBe("wss://sdr.local/api/ws");
    expect(clientEvents().at(-1)?.message).toBe("WebTransport failed, using WebSocket");

    latest().accept();
    expect(socket.transport()).toBe("WebSocket");
    FakeWebTransport.refuse = false;
    socket.retryNow();
    latest().drop();
    socket.retryNow();
    expect(sockets).toHaveLength(2);
    expect(sessions).toHaveLength(1);
    socket.close();
  });

  it("uses the WebSocket when the server offers nothing else", async () => {
    const socket = new SdrSocket("/api/ws", async () => null);
    socket.connect();
    await vi.waitFor(() => expect(sockets).toHaveLength(1));
    expect(sessions).toHaveLength(0);
    socket.close();
  });
});

describe("LateAudio", () => {
  it("drops a datagram that arrives behind a newer one, but not a restarted stream", () => {
    const late = new LateAudio();
    expect(late.isLate({ streamId: 1, timestamp: 1920n })).toBe(false);
    expect(late.isLate({ streamId: 1, timestamp: 960n })).toBe(true);
    expect(late.isLate({ streamId: 1, timestamp: 1920n })).toBe(true);
    expect(late.isLate({ streamId: 2, timestamp: 960n })).toBe(false);
    expect(late.isLate({ streamId: 1, timestamp: 2880n })).toBe(false);

    const restarted = new LateAudio();
    expect(restarted.isLate({ streamId: 1, timestamp: 480_000n })).toBe(false);
    expect(restarted.isLate({ streamId: 1, timestamp: 0n })).toBe(false);
  });
});
