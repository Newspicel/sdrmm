import { useEffect, useState } from "react";
import { Button } from "../../components/BaseControls";
import { FACE_BTN, FACE_BTN_DANGER } from "../../components/controls";
import { FaceFault } from "../../components/face/Fault";
import { FaceStats, Stat } from "../../components/face/Stats";
import { formatCount } from "../../components/format";
import type { EventOutputTarget, FeedStatus, ServerEvent } from "../../lib/types";
import { useWorkspaceSelect } from "../context";
import { FaceEmpty, FaceFooter } from "./NodeShell";

type FeedTarget = Extract<EventOutputTarget, { service: "feed" }>;

export function feedState(
  current: FeedStatus | null,
  connected: boolean,
  enabled: boolean,
): string | undefined {
  if (current?.error) {
    return undefined;
  }
  if (current?.waiting) {
    return current.waiting;
  }
  if (current?.listening) {
    return "Listening";
  }
  if (!connected) {
    return undefined;
  }
  return enabled ? "Opening server" : "Server closed";
}

export function FeedOutputControls({
  node,
  target,
  connected,
  onEdit,
}: {
  node: string;
  target: FeedTarget;
  connected: boolean;
  onEdit: (target: FeedTarget) => void;
}) {
  const socket = useWorkspaceSelect((workspace) => workspace.socket);
  const [status, setStatus] = useState<FeedStatus | null>(null);
  useEffect(() => {
    const offEvent = socket.on("event", (event: ServerEvent) => {
      if (event.type === "FeedStatus" && event.data.node === node) setStatus(event.data);
    });
    const offStatus = socket.on("status", () => setStatus(null));
    return () => {
      offEvent();
      offStatus();
    };
  }, [socket, node]);
  const enabled = target.enabled === true;
  const current = connected && enabled && status?.address === target.address ? status : null;
  return (
    <>
      <FaceEmpty hint={feedState(current, connected, enabled)} />
      {current?.error ? <FaceFault message="Server failed" detail={current.error} /> : null}
      <FaceFooter>
        {current?.listening && (
          <FaceStats>
            <Stat label="Clients" title="Clients connected to this server">
              {current.clients}
            </Stat>
            <Stat label="Frames" title="Frames sent">
              {formatCount(current.frames)}
            </Stat>
          </FaceStats>
        )}
        <Button
          type="button"
          className={enabled ? FACE_BTN_DANGER : FACE_BTN}
          disabled={!enabled && (!connected || target.address.trim() === "")}
          onClick={() => {
            setStatus(null);
            onEdit({ ...target, enabled: !enabled });
          }}
        >
          {enabled ? "Close server" : "Open server"}
        </Button>
      </FaceFooter>
    </>
  );
}
