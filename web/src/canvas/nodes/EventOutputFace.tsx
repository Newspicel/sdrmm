import { useQuery } from "@tanstack/react-query";
import { Chips, ChoiceChip, NumberChip } from "../../components/face/Chips";
import { TextChip } from "../../components/face/TextChip";
import {
  ACCOUNT_SPECS,
  accountOf,
  accountOptions,
  accountsOf,
  NO_ACCOUNT,
} from "../../lib/accounts";
import { aboutQuery, accountsQuery } from "../../lib/api";
import { portOn, useFeeds } from "../../lib/resolved";
import type {
  Account,
  EventOutputTarget,
  FeedEntry,
  PatchNode,
  PatchNodeOf,
} from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import {
  eventOutputConfigured,
  feedTarget,
  newOutputTarget,
  outputServices,
  placeOf,
  type SigningTarget,
  signsIn,
  takesMessage,
  WEBHOOK_FORMATS,
} from "./eventOutput";
import { FeedOutputControls } from "./FeedOutputControls";
import { MessageChip } from "./MessageChip";
import { FaceBody, FaceEmpty, NodeShell } from "./NodeShell";
import { OutputDelivery } from "./OutputDelivery";

const NO_ACCOUNTS: readonly Account[] = [];

export function EventOutputFace({ node }: { node: PatchNode }) {
  if (node.kind !== "event_output") {
    return null;
  }
  return <EventOutputNodeFace node={node} />;
}

function EventOutputNodeFace({ node }: { node: PatchNodeOf<"event_output"> }) {
  const workspace = useWorkspaceContext();
  const notify = useQuery(aboutQuery(true)).data?.notify === true;
  const accounts = useQuery(accountsQuery()).data ?? NO_ACCOUNTS;
  const target = node.data.target;
  const feeds = workspace.context.catalog.feeds ?? [];
  const inputs = useFeeds(node.id, "events").length;
  const configured = eventOutputConfigured(target);
  const audio = portOn(workspace.resolved, node.id, "audio", "in") !== undefined;
  const editOutput = (next: Partial<PatchNodeOf<"event_output">["data"]>) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        current.kind === "event_output"
          ? { ...current, data: { ...current.data, ...next } }
          : current,
      ),
    }));
  };
  const editTarget = (next: EventOutputTarget) => editOutput({ target: next });
  return (
    <NodeShell node={node} title="Event output" category="output">
      <FaceBody>
        <Chips>
          <ChoiceChip
            label="Service"
            title="Output service"
            value={target.service}
            options={outputServices(notify, target.service, feeds)}
            onChange={(service) => editTarget(newOutputTarget(service, feeds))}
          />
          <TargetChips target={target} feeds={feeds} accounts={accounts} onEdit={editTarget} />
          {takesMessage(target.service) && (
            <MessageChip
              value={node.data.message ?? ""}
              onCommit={(message) => editOutput({ message })}
            />
          )}
        </Chips>
        {target.service === "feed" ? (
          <FeedOutputControls
            node={node.id}
            target={target}
            connected={inputs > 0}
            onEdit={editTarget}
          />
        ) : (
          <>
            <FaceEmpty hint={emptyHint(inputs, configured, target, audio)} />
            <OutputDelivery node={node.id} />
          </>
        )}
      </FaceBody>
    </NodeShell>
  );
}

function emptyHint(inputs: number, configured: boolean, target: EventOutputTarget, audio: boolean) {
  if (inputs === 0) {
    return undefined;
  }
  if (target.service === "recordings") {
    return "One WAV per call in Recordings";
  }
  if (target.service === "desktop") {
    return "One notification per event";
  }
  if (target.service === "csv") {
    return configured ? "One row per event in Recordings" : "Enter a file name";
  }
  if (target.service === "text") {
    return configured ? "One line per event in Recordings" : "Enter a file name";
  }
  if (target.service === "tunnel") {
    return configured ? "Received IPv4 and IPv6 datagrams" : "Enter the interface name";
  }
  if (!configured) {
    return signsIn(target) && (target.account ?? "") === ""
      ? "Pick an account"
      : "Enter where to send";
  }
  if (target.service === "postgres") {
    return "One row per event";
  }
  if (target.service === "influx") {
    return "One point per event";
  }
  return audio
    ? "One send per event, with available audio"
    : "One send per event, as one JSON object";
}

type TargetOf<S extends EventOutputTarget["service"]> = Extract<EventOutputTarget, { service: S }>;

function TargetChips({
  target,
  feeds,
  accounts,
  onEdit,
}: {
  target: EventOutputTarget;
  feeds: readonly FeedEntry[];
  accounts: readonly Account[];
  onEdit: (next: EventOutputTarget) => void;
}) {
  switch (target.service) {
    case "recordings":
    case "desktop":
      return null;
    case "csv":
    case "text":
      return (
        <TextChip
          label="File"
          name={target.service === "csv" ? "CSV file name" : "Text file name"}
          title="Appended in Recordings/events. Letters, digits, - _ and ."
          value={target.file}
          onCommit={(file) => onEdit({ ...target, file })}
        />
      );
    case "feed":
      return <FeedChips target={target} feeds={feeds} onEdit={onEdit} />;
    case "tunnel":
      return <TunnelChips target={target} onEdit={onEdit} />;
    case "webhook":
    case "matrix":
    case "postgres":
    case "influx":
    case "mqtt":
      return <AccountChips target={target} accounts={accounts} onEdit={onEdit} />;
  }
}

function TunnelChips({
  target,
  onEdit,
}: {
  target: TargetOf<"tunnel">;
  onEdit: (next: EventOutputTarget) => void;
}) {
  return (
    <>
      <TextChip
        label="Interface"
        name="Network interface name"
        title="TUN interface name; macOS uses utun followed by a number. Creating an interface requires system networking privileges."
        value={target.interface}
        onCommit={(name) => onEdit({ ...target, interface: name })}
      />
      <TextChip
        label="IPv4"
        name="Interface IPv4 address"
        title="Local IPv4 address of the interface"
        value={target.address}
        onCommit={(address) => onEdit({ ...target, address })}
      />
      <NumberChip
        label="Prefix"
        title="Interface IPv4 prefix length"
        value={target.prefix}
        shown={`/${target.prefix}`}
        min={0}
        max={32}
        step={1}
        onCommit={(prefix) => onEdit({ ...target, prefix })}
      />
    </>
  );
}

/** The account an output signs in with, from the Library, and where inside it the output sends. */
function AccountChips({
  target,
  accounts,
  onEdit,
}: {
  target: SigningTarget;
  accounts: readonly Account[];
  onEdit: (next: EventOutputTarget) => void;
}) {
  const own = accountsOf(accounts, [target.service]);
  const account = accountOf(own, target.account ?? NO_ACCOUNT);
  const place = placeOf(target);
  return (
    <>
      <ChoiceChip
        label="Account"
        title={`${ACCOUNT_SPECS[target.service].label} account, from the Accounts list`}
        value={target.account ?? NO_ACCOUNT}
        options={accountOptions(own, target.account ?? NO_ACCOUNT)}
        quiet={account === undefined}
        onChange={(id) => onEdit({ ...target, account: id })}
      />
      {place !== undefined && (
        <TextChip
          label={place.label}
          title={place.title}
          value={place.value}
          onCommit={(value) => onEdit(place.set(value))}
        />
      )}
      {target.service === "webhook" && (
        <ChoiceChip
          label="Format"
          title="Webhook payload format"
          value={target.format ?? "json"}
          options={WEBHOOK_FORMATS}
          onChange={(format) => onEdit({ ...target, format })}
        />
      )}
    </>
  );
}

function FeedChips({
  target,
  feeds,
  onEdit,
}: {
  target: TargetOf<"feed">;
  feeds: readonly FeedEntry[];
  onEdit: (next: TargetOf<"feed">) => void;
}) {
  const format = target.format ?? "";
  const options = feeds.map((feed) => ({ value: feed.format, label: feed.name }));
  if (!options.some((option) => option.value === format)) {
    options.push({ value: format, label: format === "" ? "none" : format });
  }
  return (
    <>
      <ChoiceChip
        label="Format"
        title="Feed format"
        value={format}
        options={options}
        onChange={(next) => onEdit(feedTarget(feeds.find((feed) => feed.format === next)))}
      />
      <TextChip
        label="Listen"
        name="Feed listen address"
        title="TCP server address. Wire events in, then connect a client here."
        value={target.address}
        onCommit={(address) => onEdit({ ...target, address, enabled: false })}
      />
    </>
  );
}
