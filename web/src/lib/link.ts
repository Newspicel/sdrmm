export interface Closed {
  code: number;
  reason: string;
  wasClean: boolean;
}

export interface LinkHandlers {
  onOpen(): void;
  onText(text: string): void;
  onBinary(buffer: ArrayBuffer, ordered: boolean): void;
  onError(): void;
  onClose(closed: Closed): void;
}

export type Transport = "WebSocket" | "WebTransport";

export interface Link {
  readonly transport: Transport;
  isOpen(): boolean;
  send(text: string): void;
  close(): void;
}

export function openWebSocket(url: string, protocols: string[], handlers: LinkHandlers): Link {
  const ws = new WebSocket(url, protocols);
  ws.binaryType = "arraybuffer";
  ws.onopen = () => handlers.onOpen();
  ws.onerror = () => {
    handlers.onError();
    ws.close();
  };
  ws.onclose = (event: CloseEvent) => handlers.onClose(event);
  ws.onmessage = (event: MessageEvent<string | ArrayBuffer>) => {
    if (typeof event.data === "string") {
      handlers.onText(event.data);
    } else {
      handlers.onBinary(event.data, true);
    }
  };
  return {
    transport: "WebSocket",
    isOpen: () => ws.readyState === WebSocket.OPEN,
    send: (text) => ws.send(text),
    close: () => {
      ws.onopen = null;
      ws.onerror = null;
      ws.onclose = null;
      ws.onmessage = null;
      ws.close();
    },
  };
}
