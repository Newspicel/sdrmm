import { WEBTRANSPORT_LENGTH_BYTES, WEBTRANSPORT_PATH } from "./frame";
import { unhex } from "./hex";
import type { Link, LinkHandlers } from "./link";
import type { WebTransportInfo } from "./types";

const CONNECT_MS = 4000;
const MAX_MESSAGE_BYTES = 16 * 1024 * 1024;
const UTF8_ENCODER = new TextEncoder();
const UTF8_DECODER = new TextDecoder();

export function webTransportUsable(): boolean {
  return typeof WebTransport !== "undefined" && window.location.protocol === "https:";
}

export function webTransportUrl(info: WebTransportInfo, token: string | null): string {
  const query = token === null ? "" : `?token=${encodeURIComponent(token)}`;
  return `https://${window.location.hostname}:${info.port}${WEBTRANSPORT_PATH}${query}`;
}

export class MessageSplitter {
  private pending = new Uint8Array(0);

  push(chunk: Uint8Array): Uint8Array[] {
    const joined = new Uint8Array(this.pending.length + chunk.length);
    joined.set(this.pending);
    joined.set(chunk, this.pending.length);
    const messages: Uint8Array[] = [];
    let at = 0;
    while (joined.length - at >= WEBTRANSPORT_LENGTH_BYTES) {
      const length = new DataView(joined.buffer, at).getUint32(0, true);
      if (length > MAX_MESSAGE_BYTES) {
        throw new Error(`a ${length} byte message is over the limit`);
      }
      const end = at + WEBTRANSPORT_LENGTH_BYTES + length;
      if (end > joined.length) {
        break;
      }
      messages.push(joined.slice(at + WEBTRANSPORT_LENGTH_BYTES, end));
      at = end;
    }
    this.pending = joined.slice(at);
    return messages;
  }
}

export function lengthPrefixed(text: string): Uint8Array {
  const body = UTF8_ENCODER.encode(text);
  const message = new Uint8Array(WEBTRANSPORT_LENGTH_BYTES + body.length);
  new DataView(message.buffer).setUint32(0, body.length, true);
  message.set(body, WEBTRANSPORT_LENGTH_BYTES);
  return message;
}

async function eachItem<T>(readable: ReadableStream<T>, each: (item: T) => void): Promise<void> {
  const reader = readable.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      return;
    }
    each(value);
  }
}

async function eachMessage(
  readable: ReadableStream<Uint8Array>,
  each: (message: Uint8Array) => void,
): Promise<void> {
  const splitter = new MessageSplitter();
  await eachItem(readable, (chunk) => {
    for (const message of splitter.push(chunk)) {
      each(message);
    }
  });
}

function standalone(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

function within<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("timed out")), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      },
    );
  });
}

export interface Dialed {
  adopt(handlers: LinkHandlers): Link;
  abandon(): void;
}

export async function dialWebTransport(
  info: WebTransportInfo,
  token: string | null,
): Promise<Dialed> {
  const pin = unhex(info.certificate_sha256);
  if (pin === null) {
    throw new Error("the certificate hash is not hex");
  }
  const session = new WebTransport(webTransportUrl(info, token), {
    serverCertificateHashes: [{ algorithm: "sha-256", value: pin }],
  });
  try {
    await within(session.ready, CONNECT_MS);
    const control = await within(session.createBidirectionalStream(), CONNECT_MS);
    return {
      adopt: (handlers) => adopt(session, control, handlers),
      abandon: () => session.close(),
    };
  } catch (error) {
    session.close();
    throw error;
  }
}

function adopt(
  session: WebTransport,
  control: WebTransportBidirectionalStream,
  handlers: LinkHandlers,
): Link {
  let open = true;
  let heard = true;
  const closeOnce = (code: number, reason: string, wasClean: boolean): void => {
    if (!open) {
      return;
    }
    open = false;
    session.close();
    if (heard) {
      handlers.onClose({ code, reason, wasClean });
    }
  };
  const broken = (): void => closeOnce(0, "", false);
  const text = (message: Uint8Array): void => {
    if (heard) {
      handlers.onText(UTF8_DECODER.decode(message));
    }
  };
  const binary =
    (ordered: boolean) =>
    (bytes: Uint8Array): void => {
      if (heard) {
        handlers.onBinary(standalone(bytes), ordered);
      }
    };

  eachMessage(control.readable as ReadableStream<Uint8Array>, text).then(broken, broken);
  eachItem(session.datagrams.readable as ReadableStream<Uint8Array>, binary(false)).catch(broken);
  const lanes = session.incomingUnidirectionalStreams as ReadableStream<ReadableStream<Uint8Array>>;
  eachItem(lanes, (lane) => {
    eachMessage(lane, binary(true)).catch(broken);
  }).catch(broken);
  session.closed.then(
    (closed) => closeOnce(closed.closeCode ?? 0, closed.reason ?? "", true),
    broken,
  );

  const writer = control.writable.getWriter();
  return {
    transport: "WebTransport",
    isOpen: () => open,
    send: (command) => {
      writer.write(lengthPrefixed(command)).catch(broken);
    },
    close: () => {
      heard = false;
      open = false;
      session.close();
    },
  };
}
