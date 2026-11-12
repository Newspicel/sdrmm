import { getToken } from "./auth";
import { recordEvent } from "./diagnostics";
import type { AudioFrame } from "./frame";
import {
  decodeAudio,
  decodeFusionGrid,
  decodeIq,
  decodeRangeDoppler,
  decodeSpatialSpectrum,
  decodeSpectrum,
  decodeSymbols,
  decodeVideo,
  decodeVisibility,
  FRAME_KIND_AUDIO_OPUS,
  FRAME_KIND_FUSION_GRID,
  FRAME_KIND_IQ_I16,
  FRAME_KIND_RANGE_DOPPLER,
  FRAME_KIND_SPATIAL_SPECTRUM,
  FRAME_KIND_SPECTRUM,
  FRAME_KIND_SYMBOLS,
  FRAME_KIND_VIDEO_JPEG,
  FRAME_KIND_VISIBILITY,
  frameKind,
  WS_BEARER_PROTOCOL_PREFIX,
  WS_SUBPROTOCOL,
} from "./frame";
import { hexUtf8 } from "./hex";
import { type Closed, type Link, type LinkHandlers, openWebSocket } from "./link";
import {
  type Listener,
  ListenerRegistry,
  type SocketEventKind,
  type SocketEvents,
  type Unsubscribe,
} from "./listeners";
import type { SurfaceFrame, SurfaceKind } from "./surface";
import type { ClientCommand, ServerEvent, WebTransportInfo } from "./types";
import { dialWebTransport, webTransportUsable } from "./wt";

const RECONNECT_MS = 1000;
const RECONNECT_MAX_MS = 30_000;
const STABLE_MS = 10_000;
const WEBTRANSPORT_RETRY_MS = 5 * 60_000;
const REORDER_WINDOW_FRAMES = 48_000n;

export type WebTransportOffer = () => Promise<WebTransportInfo | null>;

export class LateAudio {
  private readonly newest = new Map<number, bigint>();

  isLate(frame: Pick<AudioFrame, "streamId" | "timestamp">): boolean {
    const newest = this.newest.get(frame.streamId);
    if (
      newest !== undefined &&
      frame.timestamp <= newest &&
      newest - frame.timestamp < REORDER_WINDOW_FRAMES
    ) {
      return true;
    }
    this.newest.set(frame.streamId, frame.timestamp);
    return false;
  }
}

export function socketProtocols(token: string | null): string[] {
  return token === null
    ? [WS_SUBPROTOCOL]
    : [WS_SUBPROTOCOL, WS_BEARER_PROTOCOL_PREFIX + hexUtf8(token)];
}

export class SdrSocket {
  private link: Link | null = null;
  private dialing = 0;
  private reconnectTimer: number | null = null;
  private closed = false;
  private backoffMs = RECONNECT_MS;
  private openedAt = 0;
  private webTransportAfter = 0;
  private lateAudio = new LateAudio();
  private readonly path: string;
  private readonly offer: WebTransportOffer | null;
  private readonly listeners = new ListenerRegistry();

  constructor(path = "/api/ws", offer: WebTransportOffer | null = null) {
    this.path = path;
    this.offer = offer;
  }

  transport(): Link["transport"] | null {
    return this.link?.isOpen() ? this.link.transport : null;
  }

  private url(): string {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${window.location.host}${this.path}`;
  }

  connect(): void {
    this.closed = false;
    window.addEventListener("online", this.handleOnline);
    this.open();
  }

  private readonly handleOnline = (): void => {
    this.retryNow();
  };

  send(command: ClientCommand): void {
    if (this.link?.isOpen()) {
      this.link.send(JSON.stringify(command));
    }
  }

  isConnected(): boolean {
    return this.link?.isOpen() ?? false;
  }

  on<K extends SocketEventKind>(kind: K, listener: Listener<K>): Unsubscribe {
    return this.listeners.on(kind, listener);
  }

  close(): void {
    this.closed = true;
    window.removeEventListener("online", this.handleOnline);
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.detach();
  }

  private detach(): void {
    this.dialing += 1;
    this.link?.close();
    this.link = null;
  }

  private open(): void {
    this.detach();
    const dial = this.dialing;
    const offer = this.offer;
    if (offer === null || !webTransportUsable() || Date.now() < this.webTransportAfter) {
      this.openWebSocket(dial);
      return;
    }
    void this.openWebTransport(dial, offer);
  }

  private openWebSocket(dial: number): void {
    this.link = openWebSocket(this.url(), socketProtocols(getToken()), this.handlers(dial));
  }

  private async openWebTransport(dial: number, offer: WebTransportOffer): Promise<void> {
    const info = await offer().catch(() => null);
    if (dial !== this.dialing) {
      return;
    }
    if (info === null) {
      this.openWebSocket(dial);
      return;
    }
    try {
      const dialed = await dialWebTransport(info, getToken());
      if (dial !== this.dialing) {
        dialed.abandon();
        return;
      }
      const handlers = this.handlers(dial);
      this.link = dialed.adopt(handlers);
      handlers.onOpen();
    } catch {
      recordEvent("warn", "socket", "WebTransport failed, using WebSocket");
      this.webTransportAfter = Date.now() + WEBTRANSPORT_RETRY_MS;
      if (dial === this.dialing) {
        this.openWebSocket(dial);
      }
    }
  }

  private handlers(dial: number): LinkHandlers {
    const current = (): boolean => dial === this.dialing;
    return {
      onOpen: () => {
        if (current()) {
          this.openedAt = Date.now();
          this.lateAudio = new LateAudio();
          this.listeners.emit("status", true);
        }
      },
      onText: (text) => {
        if (current()) {
          this.dispatchText(text);
        }
      },
      onBinary: (buffer, ordered) => {
        if (current()) {
          this.dispatchBinary(buffer, ordered);
        }
      },
      onError: () => {
        if (current()) {
          recordEvent("warn", "socket", "error");
        }
      },
      onClose: (closed) => {
        if (current()) {
          this.handleClose(closed);
        }
      },
    };
  }

  private handleClose(closed: Closed): void {
    if (this.openedAt !== 0) {
      recordEvent("warn", "socket", describeClose(closed, Date.now() - this.openedAt));
    }
    if (this.openedAt !== 0 && Date.now() - this.openedAt >= STABLE_MS) {
      this.backoffMs = RECONNECT_MS;
    }
    this.openedAt = 0;
    this.listeners.emit("status", false);
    this.scheduleReconnect();
  }

  private dispatchText(text: string): void {
    let event: ServerEvent;
    try {
      event = JSON.parse(text) as ServerEvent;
    } catch {
      recordEvent("error", "socket", "unreadable event");
      return;
    }
    this.listeners.emit("event", event);
  }

  private dispatchBinary(buffer: ArrayBuffer, ordered: boolean): void {
    const kind = frameKind(buffer);
    switch (kind) {
      case FRAME_KIND_SPECTRUM:
        this.emitFrame("spectrum", decodeSpectrum(buffer), kind);
        break;
      case FRAME_KIND_AUDIO_OPUS:
        this.emitAudio(decodeAudio(buffer), ordered);
        break;
      case FRAME_KIND_IQ_I16:
        this.emitFrame("iq", decodeIq(buffer), kind);
        break;
      case FRAME_KIND_SYMBOLS:
        this.emitFrame("symbols", decodeSymbols(buffer), kind);
        break;
      case FRAME_KIND_VIDEO_JPEG:
        this.emitFrame("video", decodeVideo(buffer), kind);
        break;
      case FRAME_KIND_RANGE_DOPPLER:
        this.emitSurface("range_doppler", decodeRangeDoppler(buffer), kind);
        break;
      case FRAME_KIND_SPATIAL_SPECTRUM:
        this.emitSurface("spatial_spectrum", decodeSpatialSpectrum(buffer), kind);
        break;
      case FRAME_KIND_VISIBILITY:
        this.emitSurface("visibility", decodeVisibility(buffer), kind);
        break;
      case FRAME_KIND_FUSION_GRID:
        this.emitSurface("fusion_grid", decodeFusionGrid(buffer), kind);
        break;
      default:
        recordEvent("error", "socket", `unknown frame ${kind ?? "without header"}`);
        break;
    }
  }

  private emitAudio(frame: AudioFrame | null, ordered: boolean): void {
    if (frame !== null && !ordered && this.lateAudio.isLate(frame)) {
      return;
    }
    this.emitFrame("audio", frame, FRAME_KIND_AUDIO_OPUS);
  }

  private emitFrame<K extends SocketEventKind>(
    kind: K,
    frame: SocketEvents[K] | null,
    raw: number,
  ): void {
    if (frame === null) {
      recordEvent("error", "socket", `unreadable frame ${raw}`);
      return;
    }
    this.listeners.emit(kind, frame);
  }

  private emitSurface<K extends SurfaceKind>(
    kind: K,
    frame: Extract<SurfaceFrame, { kind: K }>["frame"] | null,
    raw: number,
  ): void {
    this.emitFrame("surface", frame === null ? null : ({ kind, frame } as SurfaceFrame), raw);
  }

  private scheduleReconnect(): void {
    if (this.closed || this.reconnectTimer !== null) {
      return;
    }
    const delay = this.backoffMs * (0.5 + Math.random() * 0.5);
    this.backoffMs = Math.min(this.backoffMs * 2, RECONNECT_MAX_MS);
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.closed) {
        this.open();
      }
    }, delay);
  }

  retryNow(): void {
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.backoffMs = RECONNECT_MS;
    if (!this.closed && !this.isConnected()) {
      this.open();
    }
  }
}

export function describeClose(event: Closed, openMs: number): string {
  const reason = event.reason.length > 0 ? ` ${event.reason}` : "";
  const clean = event.wasClean ? "" : ", unclean";
  const page = typeof document === "undefined" ? "" : `, page ${document.visibilityState}`;
  return `closed ${event.code}${reason} after ${(openMs / 1000).toFixed(1)} s${clean}${page}`;
}
