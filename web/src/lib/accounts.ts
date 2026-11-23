import type { Options } from "../components/controls";
import { ACCOUNT_LIMITS } from "./limits";
import type { Account, Secret } from "./types";

export type AccountKind = Account["service"];

/** One setting of an account, as its form shows it. */
export interface AccountField {
  key: string;
  label: string;
  title: string;
  /** A credential: typed in, kept on the server, shown only as set. */
  secret?: boolean;
  /** The account works without it, so a set one may be cleared. */
  optional?: boolean;
  maxLength?: number;
  /** Shown beside the service in the account list. */
  listed?: boolean;
  /** A closed set of values, picked from a dropdown. */
  choices?: Options<string>;
  /** The value a new account starts with. */
  initial?: string;
  /** Shown only while the field `key` holds one of these values. */
  when?: { key: string; is: string | readonly string[] };
}

export interface AccountSpec {
  label: string;
  title: string;
  fields: readonly AccountField[];
}

const URL_LEN = ACCOUNT_LIMITS.url_len;

export const ACCOUNT_SPECS: Readonly<Record<AccountKind, AccountSpec>> = {
  discord: {
    label: "Discord",
    title: "Discord bot",
    fields: [
      {
        key: "nickname",
        label: "Nickname",
        title: "Name shown in each server. Empty keeps the bot's name",
        maxLength: ACCOUNT_LIMITS.discord_nickname_len,
      },
      { key: "token", label: "Token", title: "Discord bot token", secret: true },
    ],
  },
  teamspeak: {
    label: "TeamSpeak",
    title: "TeamSpeak client",
    fields: [
      {
        key: "address",
        label: "Server",
        title: "TeamSpeak server, host or host:port",
        maxLength: URL_LEN,
        listed: true,
      },
      {
        key: "nickname",
        label: "Nickname",
        title: "Name shown in the channel",
        maxLength: ACCOUNT_LIMITS.name_len,
      },
      {
        key: "password",
        label: "Password",
        title: "TeamSpeak server password",
        secret: true,
        optional: true,
      },
      {
        key: "identity",
        label: "Identity",
        title: "TeamSpeak identity. Made on save when empty",
        secret: true,
      },
    ],
  },
  webhook: {
    label: "Webhook",
    title: "HTTPS endpoint",
    fields: [{ key: "url", label: "Endpoint", title: "Webhook URL", secret: true }],
  },
  matrix: {
    label: "Matrix",
    title: "Matrix bot",
    fields: [
      {
        key: "homeserver_url",
        label: "Homeserver",
        title: "Matrix homeserver URL",
        maxLength: URL_LEN,
        listed: true,
      },
      {
        key: "login",
        label: "Login",
        title: "A token only sends events. Voice needs a password or OAuth sign-in",
        choices: [
          { value: "token", label: "Token" },
          { value: "password", label: "Password" },
          { value: "oauth", label: "OAuth" },
        ],
        initial: "token",
      },
      {
        key: "user",
        label: "User",
        title: "Matrix user id, e.g. @radio:example.org",
        maxLength: ACCOUNT_LIMITS.name_len,
        when: { key: "login", is: ["password", "oauth"] },
      },
      {
        key: "access_token",
        label: "Token",
        title: "Matrix access token",
        secret: true,
        when: { key: "login", is: "token" },
      },
      {
        key: "password",
        label: "Password",
        title: "Matrix account password",
        secret: true,
        when: { key: "login", is: "password" },
      },
      {
        key: "nickname",
        label: "Nickname",
        title: "Name shown in the room. Empty keeps the account's name",
        maxLength: ACCOUNT_LIMITS.name_len,
      },
      {
        key: "recovery_key",
        label: "Recovery key",
        title: "Unlocks room keys saved on the homeserver, so earlier messages decrypt",
        secret: true,
        optional: true,
        when: { key: "login", is: ["password", "oauth"] },
      },
    ],
  },
  mqtt: {
    label: "MQTT",
    title: "MQTT broker",
    fields: [
      {
        key: "broker_url",
        label: "Broker",
        title: "MQTT broker URL",
        maxLength: URL_LEN,
        listed: true,
      },
      { key: "username", label: "User", title: "MQTT username" },
      { key: "password", label: "Password", title: "MQTT password", secret: true, optional: true },
    ],
  },
  postgres: {
    label: "PostgreSQL",
    title: "PostgreSQL database",
    fields: [
      {
        key: "url",
        label: "Server",
        title: "postgres://host:5432/database. Add ?sslmode=disable for a server without TLS",
        maxLength: URL_LEN,
        listed: true,
      },
      { key: "username", label: "User", title: "PostgreSQL username" },
      {
        key: "password",
        label: "Password",
        title: "PostgreSQL password",
        secret: true,
        optional: true,
      },
    ],
  },
  influx: {
    label: "InfluxDB",
    title: "InfluxDB 2 or 3",
    fields: [
      {
        key: "url",
        label: "Server",
        title: "InfluxDB base URL, e.g. http://127.0.0.1:8086",
        maxLength: URL_LEN,
        listed: true,
      },
      { key: "org", label: "Org", title: "InfluxDB organization" },
      { key: "token", label: "Token", title: "InfluxDB token", secret: true, optional: true },
    ],
  },
};

export const ACCOUNT_KINDS: readonly AccountKind[] = [
  "discord",
  "teamspeak",
  "matrix",
  "webhook",
  "mqtt",
  "postgres",
  "influx",
];

/** The services a voice chat joins through. */
export const CHAT_KINDS: readonly AccountKind[] = ["discord", "teamspeak", "matrix"];

export const NO_ACCOUNT = "";

export function serviceOptions(
  kinds: readonly AccountKind[] = ACCOUNT_KINDS,
): Options<AccountKind> {
  return kinds.map((kind) => ({
    value: kind,
    label: ACCOUNT_SPECS[kind].label,
    title: ACCOUNT_SPECS[kind].title,
  }));
}

export function fieldOf(account: Account, key: string): unknown {
  return (account as Readonly<Record<string, unknown>>)[key];
}

export function withField(account: Account, key: string, value: unknown): Account {
  return { ...account, [key]: value };
}

/** A blank account of `service`: each plain setting empty, each credential unset. */
export function newAccount(service: AccountKind): Account {
  const fields: Record<string, unknown> = { id: "", name: "", service };
  for (const field of ACCOUNT_SPECS[service].fields) {
    if (field.secret !== true) {
      fields[field.key] = field.initial ?? "";
    }
  }
  return fields as Account;
}

export function fieldShown(account: Account, field: AccountField): boolean {
  if (field.when === undefined) {
    return true;
  }
  const value = fieldOf(account, field.when.key);
  const wanted = field.when.is;
  return (
    typeof value === "string" &&
    (typeof wanted === "string" ? value === wanted : wanted.includes(value))
  );
}

/** True for a Matrix account that signs in through the homeserver's OAuth page. */
export function needsOAuth(account: Account): boolean {
  return account.service === "matrix" && account.login === "oauth";
}

function signInText(account: Account): string[] {
  if (account.service !== "matrix" || (account.login ?? "token") === "token") {
    return [];
  }
  return [secretSet(account.session) ? "signed in" : "not signed in"];
}

/** The service, then the address the account reaches. */
export function describeAccount(account: Account): string {
  const spec = ACCOUNT_SPECS[account.service];
  const listed = spec.fields
    .filter((field) => field.listed === true)
    .map((field) => fieldOf(account, field.key))
    .filter((value): value is string => typeof value === "string" && value !== "");
  return [spec.label, ...listed, ...signInText(account)].join(" · ");
}

export function secretSet(secret: Secret | undefined): boolean {
  if (secret === undefined) {
    return false;
  }
  return typeof secret === "string" ? secret.trim() !== "" : secret.secret !== "";
}

export function accountsOf(
  accounts: readonly Account[],
  kinds: readonly AccountKind[],
): readonly Account[] {
  return accounts.filter((account) => kinds.includes(account.service));
}

/** The account a node names, or none when this server does not know the id. */
export function accountOf(accounts: readonly Account[], id: string): Account | undefined {
  return accounts.find((account) => account.id === id);
}

/**
 * The accounts a node can pick. A picked id missing from the list (deleted, or hidden from a
 * non-admin) stays shown as unknown, so a set account never reads as none.
 */
export function accountOptions(
  accounts: readonly Account[],
  picked: string = NO_ACCOUNT,
): Options<string> {
  const unknown =
    picked !== NO_ACCOUNT && accountOf(accounts, picked) === undefined
      ? [{ value: picked, label: "unknown", title: "Not in the account list you can see" }]
      : [];
  return [
    { value: NO_ACCOUNT, label: "none" },
    ...unknown,
    ...accounts.map((account) => ({
      value: account.id,
      label: account.name,
      title: ACCOUNT_SPECS[account.service].label,
    })),
  ];
}
