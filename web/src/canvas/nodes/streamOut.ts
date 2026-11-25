import { useEffect, useState } from "react";
import { withToken } from "../../lib/auth";
import type { ServerEvent, StreamOutStatus } from "../../lib/types";
import { useWorkspaceSelect } from "../context";

/** The address a player opens for a stream served at `path`. */
export function streamUrl(path: string, origin: string): string {
  return new URL(withToken(path), origin).href;
}

export function pictureSize(status: StreamOutStatus | null): string {
  return status === null || status.width === 0 ? "—" : `${status.width}×${status.height}`;
}

/** The node's latest status, cleared when the socket reconnects. */
export function useStreamOutStatus(node: string): StreamOutStatus | null {
  const socket = useWorkspaceSelect((workspace) => workspace.socket);
  const [status, setStatus] = useState<StreamOutStatus | null>(null);
  useEffect(() => {
    const offEvent = socket.on("event", (event: ServerEvent) => {
      if (event.type === "StreamOutStatus" && event.data.node === node) setStatus(event.data);
    });
    const offStatus = socket.on("status", () => setStatus(null));
    return () => {
      offEvent();
      offStatus();
    };
  }, [socket, node]);
  return status;
}
