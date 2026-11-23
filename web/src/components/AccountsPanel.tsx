import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import { secretSet } from "../canvas/nodes/eventOutput";
import { newChatAccount, SERVICE_LABEL, SERVICES } from "../canvas/nodes/voiceChat";
import {
  CHAT_ACCOUNTS_KEY,
  chatAccountsQuery,
  createChatAccount,
  deleteChatAccount,
  updateChatAccount,
} from "../lib/api";
import { VOICE_CHAT_LIMITS } from "../lib/limits";
import { pushToast } from "../lib/toasts";
import type { ChatAccount, Secret } from "../lib/types";
import { Button } from "./BaseControls";
import { ConfirmButton } from "./ConfirmButton";
import { BTN, BTN_PRIMARY } from "./controls";
import { ChipRows, Chips } from "./face/Chips";
import { TextChip } from "./face/TextChip";
import { List, ListRow, Panel, PanelHint, PanelToolbar, RowAction } from "./ListPanel";

const NAME_LEN = VOICE_CHAT_LIMITS.account_len;

export interface AccountActions {
  draft: (account: ChatAccount | null) => void;
  save: (account: ChatAccount) => void;
  remove: (id: string) => void;
}

export interface AccountsViewProps {
  accounts: readonly ChatAccount[] | undefined;
  failed: boolean;
  draft: ChatAccount | null;
  busy: boolean;
  on: AccountActions;
}

export function AccountsPanel() {
  const queryClient = useQueryClient();
  const accounts = useQuery(chatAccountsQuery());
  const [draft, setDraft] = useState<ChatAccount | null>(null);
  const refresh = (): void => {
    void queryClient.invalidateQueries({ queryKey: CHAT_ACCOUNTS_KEY });
  };
  const save = useMutation({
    mutationFn: (account: ChatAccount) =>
      account.id === "" ? createChatAccount(account) : updateChatAccount(account),
    onSuccess: () => setDraft(null),
    onError: (error) => pushToast(error.message),
    onSettled: refresh,
  });
  const remove = useMutation({
    mutationFn: deleteChatAccount,
    onSuccess: (_, id) => setDraft((current) => (current?.id === id ? null : current)),
    onError: (error) => pushToast(error.message),
    onSettled: refresh,
  });
  return (
    <AccountsView
      accounts={accounts.data}
      failed={accounts.isError}
      draft={draft}
      busy={save.isPending || remove.isPending}
      on={{
        draft: setDraft,
        save: (account) => save.mutate(account),
        remove: (id) => remove.mutate(id),
      }}
    />
  );
}

function ListFailed() {
  return (
    <p role="alert" className="text-xs text-danger">
      Account list failed
    </p>
  );
}

export function AccountsView({ accounts, failed, draft, busy, on }: AccountsViewProps) {
  if (accounts === undefined) {
    return <Panel>{failed ? <ListFailed /> : <PanelHint>Loading</PanelHint>}</Panel>;
  }
  const adding = draft !== null && draft.id === "";
  return (
    <Panel>
      {failed && <ListFailed />}
      <PanelToolbar>
        {SERVICES.map((service) => (
          <Button
            key={service.value}
            type="button"
            className={BTN}
            title={service.title}
            disabled={draft !== null}
            onClick={() => on.draft(newChatAccount(service.value))}
          >
            Add {service.label}
          </Button>
        ))}
      </PanelToolbar>
      {adding && <AccountForm draft={draft} busy={busy} on={on} />}
      {accounts.length === 0 ? (
        <PanelHint>No accounts</PanelHint>
      ) : (
        <List>
          {accounts.map((account) => (
            <AccountRow
              key={account.id}
              account={account}
              draft={draft?.id === account.id ? draft : null}
              editing={draft !== null}
              busy={busy}
              on={on}
            />
          ))}
        </List>
      )}
    </Panel>
  );
}

function describe(account: ChatAccount): string {
  return account.service === "teamspeak"
    ? `${SERVICE_LABEL.teamspeak} · ${account.address}`
    : SERVICE_LABEL.discord;
}

function AccountRow({
  account,
  draft,
  editing,
  busy,
  on,
}: {
  account: ChatAccount;
  draft: ChatAccount | null;
  /** Some account's form is open, so no other may open and drop its edits. */
  editing: boolean;
  busy: boolean;
  on: AccountActions;
}) {
  return (
    <ListRow
      primary={account.name}
      secondary={describe(account)}
      actions={
        <>
          <RowAction
            label={`Edit ${account.name}`}
            glyph={Pencil}
            disabled={editing}
            onClick={() => on.draft(account)}
          />
          <ConfirmButton
            label={`Delete ${account.name}`}
            size="icon"
            icon={Trash2}
            disabled={busy}
            onConfirm={() => on.remove(account.id)}
          />
        </>
      }
    >
      {draft !== null && <AccountForm draft={draft} busy={busy} on={on} />}
    </ListRow>
  );
}

function AccountForm({
  draft,
  busy,
  on,
}: {
  draft: ChatAccount;
  busy: boolean;
  on: AccountActions;
}) {
  return (
    <div className="flex flex-col gap-3 rounded-control border border-line p-2">
      <span className="text-xs font-medium text-ink">{SERVICE_LABEL[draft.service]}</span>
      <ChipRows>
        <Chips inset="none">
          <TextChip
            label="Name"
            title="Account name"
            value={draft.name}
            maxLength={NAME_LEN}
            onCommit={(name) => on.draft({ ...draft, name })}
          />
          <ServiceFields draft={draft} onDraft={on.draft} />
        </Chips>
      </ChipRows>
      <span className="flex gap-2">
        <Button
          type="button"
          className={BTN_PRIMARY}
          disabled={busy || draft.name.trim() === ""}
          onClick={() => on.save(draft)}
        >
          Save
        </Button>
        <Button type="button" className={BTN} onClick={() => on.draft(null)}>
          Cancel
        </Button>
      </span>
    </div>
  );
}

function ServiceFields({
  draft,
  onDraft,
}: {
  draft: ChatAccount;
  onDraft: (account: ChatAccount) => void;
}) {
  if (draft.service === "discord") {
    return (
      <SecretField
        label="Token"
        title="Discord bot token, kept on this server"
        secret={draft.token}
        onSet={(token) => onDraft({ ...draft, token })}
      />
    );
  }
  return (
    <>
      <TextChip
        label="Server"
        title="TeamSpeak server, host or host:port"
        value={draft.address}
        onCommit={(address) => onDraft({ ...draft, address })}
      />
      <TextChip
        label="Nickname"
        title="Name shown in the channel"
        value={draft.nickname ?? ""}
        maxLength={NAME_LEN}
        onCommit={(nickname) => onDraft({ ...draft, nickname })}
      />
      <SecretField
        label="Password"
        title="TeamSpeak server password, kept on this server"
        secret={draft.password}
        onSet={(password) => onDraft({ ...draft, password })}
        onClear={() => onDraft({ ...draft, password: "" })}
      />
      <SecretField
        label="Identity"
        title="TeamSpeak identity, kept on this server. Made on save when empty"
        secret={draft.identity}
        onSet={(identity) => onDraft({ ...draft, identity })}
      />
    </>
  );
}

/**
 * A credential typed in place; the server moves it to its store and answers with an id. An
 * optional one offers Clear while set: saving it empty drops the stored value.
 */
function SecretField({
  label,
  title,
  secret,
  onSet,
  onClear,
}: {
  label: string;
  title: string;
  secret: Secret | undefined;
  onSet: (next: Secret) => void;
  onClear?: () => void;
}) {
  const set = secretSet(secret);
  return (
    <>
      <TextChip
        label={label}
        title={title}
        value={typeof secret === "string" ? secret : ""}
        placeholder={set ? "set" : "none"}
        secret
        onCommit={onSet}
      />
      {onClear !== undefined && set && (
        <Button
          type="button"
          className={BTN}
          aria-label={`Clear ${label.toLowerCase()}`}
          title={`Drop the ${label.toLowerCase()} on save`}
          onClick={onClear}
        >
          Clear
        </Button>
      )}
    </>
  );
}
