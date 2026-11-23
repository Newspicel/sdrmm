import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  ACCOUNT_SPECS,
  type AccountField,
  type AccountKind,
  describeAccount,
  fieldOf,
  fieldShown,
  needsOAuth,
  newAccount,
  secretSet,
  serviceOptions,
  withField,
  withoutHidden,
} from "../lib/accounts";
import {
  ACCOUNTS_KEY,
  accountsQuery,
  createAccount,
  deleteAccount,
  signInKey,
  signInQuery,
  startSignIn,
  updateAccount,
} from "../lib/api";
import { ACCOUNT_LIMITS } from "../lib/limits";
import { pushToast } from "../lib/toasts";
import type { Account, MatrixSignInState, Secret } from "../lib/types";
import { Button } from "./BaseControls";
import { ConfirmButton } from "./ConfirmButton";
import { BTN, BTN_PRIMARY, type Options } from "./controls";
import { ChipRows, Chips, ChoiceChip } from "./face/Chips";
import { TextChip } from "./face/TextChip";
import { List, ListRow, Panel, PanelHint, PanelToolbar, RowAction } from "./ListPanel";
import { Select } from "./Select";

const ADD = "";
const ADD_OPTIONS: Options<AccountKind | typeof ADD> = [
  { value: ADD, label: "Add", disabled: true },
  ...serviceOptions(),
];

export interface AccountActions {
  draft: (account: Account | null) => void;
  save: (account: Account) => void;
  remove: (id: string) => void;
}

export interface AccountsViewProps {
  accounts: readonly Account[] | undefined;
  failed: boolean;
  draft: Account | null;
  busy: boolean;
  on: AccountActions;
}

export function AccountsPanel() {
  const queryClient = useQueryClient();
  const accounts = useQuery(accountsQuery());
  const [draft, setDraft] = useState<Account | null>(null);
  const refresh = (): void => {
    void queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY });
  };
  const save = useMutation({
    mutationFn: (account: Account) =>
      account.id === "" ? createAccount(account) : updateAccount(account),
    onSuccess: () => setDraft(null),
    onError: (error) => pushToast(error.message),
    onSettled: refresh,
  });
  const remove = useMutation({
    mutationFn: deleteAccount,
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
        <Select
          label="Add account"
          value={ADD}
          options={ADD_OPTIONS}
          disabled={draft !== null}
          onChange={(service) => {
            if (service !== ADD) {
              on.draft(newAccount(service));
            }
          }}
        />
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

function AccountRow({
  account,
  draft,
  editing,
  busy,
  on,
}: {
  account: Account;
  draft: Account | null;
  /** Some account's form is open, so no other may open and drop its edits. */
  editing: boolean;
  busy: boolean;
  on: AccountActions;
}) {
  return (
    <ListRow
      primary={account.name}
      secondary={describeAccount(account)}
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
      {needsOAuth(account) && account.id !== "" && <SignIn id={account.id} />}
      {draft !== null && <AccountForm draft={draft} busy={busy} on={on} />}
    </ListRow>
  );
}

function SignIn({ id }: { id: string }) {
  const queryClient = useQueryClient();
  const state = useQuery(signInQuery(id, true));
  const start = useMutation({
    mutationFn: () => startSignIn(id),
    onError: (error) => pushToast(error.message),
    onSettled: () => queryClient.invalidateQueries({ queryKey: signInKey(id) }),
  });
  const pending = state.data?.state === "pending";
  const wasPending = useRef(false);
  useEffect(() => {
    if (wasPending.current && !pending) {
      void queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY });
    }
    wasPending.current = pending;
  }, [pending, queryClient]);
  return (
    <SignInView
      state={state.data ?? { state: "idle" }}
      busy={start.isPending}
      onStart={() => start.mutate()}
    />
  );
}

function webAddress(uri: string): boolean {
  return URL.canParse(uri) && ["http:", "https:"].includes(new URL(uri).protocol);
}

function VerificationLink({ uri }: { uri: string }) {
  if (!webAddress(uri)) {
    return <span>{uri}</span>;
  }
  return (
    <a
      href={uri}
      target="_blank"
      rel="noreferrer"
      className="text-accent underline"
      title="Open, then enter the code"
    >
      {uri}
    </a>
  );
}

export function SignInView({
  state,
  busy,
  onStart,
}: {
  state: MatrixSignInState;
  busy: boolean;
  onStart: () => void;
}) {
  if (state.state === "pending") {
    return (
      <span className="flex flex-wrap items-center gap-2 text-xs text-ink">
        <VerificationLink uri={state.verification_uri} />
        <code className="font-mono" title="Code to enter">
          {state.user_code}
        </code>
      </span>
    );
  }
  return (
    <span className="flex items-center gap-2">
      <Button
        type="button"
        className={BTN}
        disabled={busy}
        title="Sign in through the homeserver's OAuth page"
        onClick={onStart}
      >
        Sign in
      </Button>
      {state.state === "failed" && (
        <span role="alert" className="text-xs text-danger">
          {state.error}
        </span>
      )}
    </span>
  );
}

function AccountForm({ draft, busy, on }: { draft: Account; busy: boolean; on: AccountActions }) {
  return (
    <div className="flex flex-col gap-3 rounded-control border border-line p-2">
      <span className="text-xs font-medium text-ink">{ACCOUNT_SPECS[draft.service].label}</span>
      <ChipRows>
        <Chips inset="none">
          <TextChip
            label="Name"
            title="Account name"
            value={draft.name}
            maxLength={ACCOUNT_LIMITS.name_len}
            onCommit={(name) => on.draft({ ...draft, name })}
          />
          {ACCOUNT_SPECS[draft.service].fields
            .filter((field) => fieldShown(draft, field))
            .map((field) => (
              <FieldChip key={field.key} draft={draft} field={field} onDraft={on.draft} />
            ))}
        </Chips>
      </ChipRows>
      <span className="flex gap-2">
        <Button
          type="button"
          className={BTN_PRIMARY}
          disabled={busy || draft.name.trim() === ""}
          onClick={() => on.save(withoutHidden(draft))}
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

function FieldChip({
  draft,
  field,
  onDraft,
}: {
  draft: Account;
  field: AccountField;
  onDraft: (account: Account) => void;
}) {
  const value = fieldOf(draft, field.key);
  const set = (next: unknown): void => onDraft(withField(draft, field.key, next));
  if (field.choices !== undefined) {
    return (
      <ChoiceChip
        label={field.label}
        title={field.title}
        value={typeof value === "string" ? value : (field.initial ?? "")}
        options={field.choices}
        onChange={set}
      />
    );
  }
  if (field.secret === true) {
    return (
      <SecretField
        label={field.label}
        title={`${field.title}, kept on this server`}
        secret={value as Secret | undefined}
        onSet={set}
        onClear={field.optional === true ? () => set("") : undefined}
      />
    );
  }
  return (
    <TextChip
      label={field.label}
      title={field.title}
      value={typeof value === "string" ? value : ""}
      maxLength={field.maxLength ?? ACCOUNT_LIMITS.url_len}
      onCommit={set}
    />
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
        maxLength={ACCOUNT_LIMITS.secret_len}
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
