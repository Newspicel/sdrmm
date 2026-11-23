import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Button } from "../../components/BaseControls";
import { Chips, ChoiceChip } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { MeterBar, MeterRow } from "../../components/face/Meter";
import { TextChip } from "../../components/face/TextChip";
import { ListChip } from "../../components/ListChip";
import {
  ACCOUNT_SPECS,
  accountOf,
  accountOptions,
  accountsOf,
  CHAT_KINDS,
} from "../../lib/accounts";
import { accountsQuery } from "../../lib/api";
import { copyText } from "../../lib/copyText";
import { VOICE_CHAT_LIMITS } from "../../lib/limits";
import { pushToast, toastError } from "../../lib/toasts";
import type {
  Account,
  ChatTalker,
  PatchNode,
  PatchNodeOf,
  ServerEvent,
  VoiceChatStatus,
} from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { GateControls, gateUnit } from "./GateControls";
import { FaceBody, NodeShell } from "./NodeShell";
import {
  ALLOWED_ITEM,
  allowedEntries,
  allowedOf,
  channelTitle,
  type GateDirection,
  KEYLESS_SEND_MODES,
  STATE_TEXT,
  withGate,
  withVoiceChat,
} from "./voiceChat";

const NO_ACCOUNTS: readonly Account[] = [];
const NO_TALKERS: readonly ChatTalker[] = [];

export function VoiceChatFace({ node }: { node: PatchNode }) {
  if (node.kind !== "voice_chat") {
    return null;
  }
  return <VoiceChatNodeFace node={node} />;
}

/** The latest status the server sent for `node`; none again after the socket reconnects. */
function useVoiceChatStatus(node: string): VoiceChatStatus | null {
  const { socket } = useWorkspaceContext();
  const [status, setStatus] = useState<VoiceChatStatus | null>(null);
  useEffect(() => {
    const offEvent = socket.on("event", (event: ServerEvent) => {
      if (event.type === "VoiceChatStatus" && event.data.node === node) {
        setStatus(event.data);
      }
    });
    const offStatus = socket.on("status", () => setStatus(null));
    return () => {
      offEvent();
      offStatus();
    };
  }, [socket, node]);
  return status;
}

function VoiceChatNodeFace({ node }: { node: PatchNodeOf<"voice_chat"> }) {
  const workspace = useWorkspaceContext();
  const accounts = accountsOf(useQuery(accountsQuery()).data ?? NO_ACCOUNTS, CHAT_KINDS);
  const status = useVoiceChatStatus(node.id);
  const account = accountOf(accounts, node.data.account);
  const edit = (change: (current: PatchNode) => PatchNode): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, change),
    }));
  const gate = (direction: GateDirection, name: string) => (
    <InspectSection label={name}>
      <Chips inset="none">
        <GateControls
          name={name}
          gate={node.data[direction]}
          modes={KEYLESS_SEND_MODES}
          volume
          levelDb={direction === "from_channel" ? status?.from_channel_db : status?.to_channel_db}
          open={
            (direction === "from_channel" ? status?.from_channel_open : status?.to_channel_open) ===
            true
          }
          onEdit={(next) => edit((current) => withGate(current, direction, next))}
        />
      </Chips>
    </InspectSection>
  );

  return (
    <NodeShell
      node={node}
      title="Voice chat"
      category="output"
      subtitle={status === null ? undefined : STATE_TEXT[status.state]}
      badge={account === undefined ? undefined : ACCOUNT_SPECS[account.service].label}
    >
      <FaceBody>
        <div className="flex flex-col px-2 py-2">
          <GateMeter
            label="To channel"
            title="Patch audio going into the channel"
            db={status?.to_channel_db}
            open={status?.to_channel_open === true}
          />
          <GateMeter
            label="From channel"
            title="Channel voices going to the transmitter"
            db={status?.from_channel_db}
            open={status?.from_channel_open === true}
          />
        </div>
        <Talkers talking={status?.talking ?? NO_TALKERS} />
        {status?.error != null && <FaceFault message="Voice chat failed" detail={status.error} />}
      </FaceBody>
      <Inspect node={node.id}>
        <InspectSection>
          <Chips inset="none">
            <ChoiceChip
              label="Account"
              title="Chat account, from the Accounts list"
              value={node.data.account}
              options={accountOptions(accounts, node.data.account)}
              quiet={account === undefined}
              onChange={(id) => edit((current) => withVoiceChat(current, { account: id }))}
            />
            <TextChip
              label="Channel"
              title={channelTitle(account)}
              value={node.data.channel}
              maxLength={VOICE_CHAT_LIMITS.channel_len}
              onCommit={(channel) => edit((current) => withVoiceChat(current, { channel }))}
            />
            <ListChip
              label="Allowed"
              title="User ids that may key the transmitter. Empty: anyone"
              item={ALLOWED_ITEM}
              entries={allowedEntries(node.data.allowed)}
              empty="anyone"
              quiet={node.data.allowed.length === 0}
              max={VOICE_CHAT_LIMITS.allowed}
              onChange={(entries) =>
                edit((current) => withVoiceChat(current, { allowed: allowedOf(entries) }))
              }
            />
          </Chips>
        </InspectSection>
        {gate("from_channel", "From channel")}
        {gate("to_channel", "To channel")}
      </Inspect>
    </NodeShell>
  );
}

function formatDb(db: number | null | undefined): string {
  return db == null || !Number.isFinite(db) ? "quiet" : `${db.toFixed(0)} dB`;
}

function GateMeter({
  label,
  title,
  db,
  open,
}: {
  label: string;
  title: string;
  db: number | null | undefined;
  open: boolean;
}) {
  return (
    <MeterRow
      label={label}
      title={title}
      solo
      meter={
        <MeterBar
          label={`${label} level`}
          value={gateUnit(db)}
          valueText={formatDb(db)}
          fill={open ? "bg-accent" : "bg-accent-dim"}
        />
      }
      readout={<span className={open ? "" : "text-ink-faint"}>{formatDb(db)}</span>}
    />
  );
}

function Talkers({ talking }: { talking: readonly ChatTalker[] }) {
  return (
    <div className="flex flex-col gap-1 border-t border-line px-2 py-1.5">
      <span className="legend" title="Who is talking in the channel now">
        Talking
      </span>
      {talking.length === 0 ? (
        <span className="text-xs text-ink-faint">none</span>
      ) : (
        talking.map((talker) => <TalkerRow key={talker.id} talker={talker} />)
      )}
    </div>
  );
}

/** One talker. A stand-in id the chat has not tied to a user offers no copy: no list can use it. */
export function TalkerRow({ talker }: { talker: ChatTalker }) {
  return (
    <div className="flex min-w-0 items-center gap-2 text-xs">
      <span
        aria-hidden
        className={`size-2 shrink-0 rounded-full ${talker.allowed ? "bg-ok" : "bg-ink-faint"}`}
      />
      <span
        className={`min-w-0 truncate ${talker.allowed ? "text-ink" : "text-ink-dim"}`}
        title={talker.allowed ? "Allowed on air" : "Not allowed on air"}
      >
        {talker.name}
      </span>
      {talker.verified && (
        <Button
          type="button"
          className="nodrag ml-auto min-w-0 truncate font-mono text-2xs text-ink-dim hover:text-accent"
          title="Copy id"
          aria-label={`Copy ${talker.name} id`}
          onClick={() => {
            copyText(talker.id).then(() => pushToast("Id copied", "info"), toastError);
          }}
        >
          {talker.id}
        </Button>
      )}
    </div>
  );
}
