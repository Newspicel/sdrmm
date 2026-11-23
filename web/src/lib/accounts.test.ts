import { describe, expect, it } from "vitest";
import {
  ACCOUNT_KINDS,
  ACCOUNT_SPECS,
  accountOf,
  accountOptions,
  accountsOf,
  CHAT_KINDS,
  describeAccount,
  fieldShown,
  NO_ACCOUNT,
  newAccount,
  secretSet,
  serviceOptions,
  withField,
} from "./accounts";
import type { Account } from "./types";

const DISCORD: Account = {
  id: "a1",
  name: "Club bot",
  service: "discord",
  token: { secret: "s1" },
};
const TEAMSPEAK: Account = {
  id: "a2",
  name: "Net",
  service: "teamspeak",
  address: "ts.example",
  nickname: "sdr",
  identity: { secret: "s2" },
};
const MATRIX: Account = {
  id: "a3",
  name: "Room",
  service: "matrix",
  homeserver_url: "https://matrix.example",
  access_token: { secret: "s3" },
};
const ACCOUNTS: Account[] = [DISCORD, TEAMSPEAK, MATRIX];

function shown = (login: string): string[] {
  return   ACCOUNT_SPECS.matrix.fields
    .filter((field) => fieldShown({ ...MATRIX, login } as Account, field))
    .map((field) => field.key);
}

describe("accounts", () => {
  it("shows an id this server does not know as no account", () => {
    expect(accountOf(ACCOUNTS, "a2")?.name).toBe("Net");
    expect(accountOf(ACCOUNTS, "gone")).toBeUndefined();
    expect(accountOptions(ACCOUNTS).map((option) => option.value)).toEqual([
      NO_ACCOUNT,
      "a1",
      "a2",
      "a3",
    ]);
  });

  it("keeps a picked id it cannot see as unknown", () => {
    const options = accountOptions(ACCOUNTS, "hidden");
    expect(options.map((option) => option.value)).toEqual([NO_ACCOUNT, "hidden", "a1", "a2", "a3"]);
    expect(options[1]?.label).toBe("unknown");
    expect(accountOptions(ACCOUNTS, "a1")).toHaveLength(4);
  });

  it("offers a voice chat only chat accounts", () => {
    expect(accountsOf(ACCOUNTS, CHAT_KINDS).map((account) => account.id)).toEqual([
      "a1",
      "a2",
      "a3",
    ]);
    expect(accountsOf(ACCOUNTS, ["webhook"])).toEqual([]);
  });

  it("starts each service with its plain settings empty and no credential", () => {
    expect(newAccount("teamspeak")).toEqual({
      id: "",
      name: "",
      service: "teamspeak",
      address: "",
      nickname: "",
    });
    for (const kind of ACCOUNT_KINDS) {
      const blank = newAccount(kind);
      expect(blank.service).toBe(kind);
      for (const field of ACCOUNT_SPECS[kind].fields) {
        expect(field.key in blank).toBe(field.secret !== true);
      }
    }
  });

  it("describes an account by its service and address, never its secret", () => {
    expect(describeAccount(DISCORD)).toBe("Discord");
    expect(describeAccount(TEAMSPEAK)).toBe("TeamSpeak · ts.example");
    expect(describeAccount(MATRIX)).toBe("Matrix · https://matrix.example");
  });

  it("edits one field by its key", () => {
    expect(withField(MATRIX, "homeserver_url", "https://m.example")).toMatchObject({
      service: "matrix",
      homeserver_url: "https://m.example",
      access_token: { secret: "s3" },
    });
  });

  it("lists every service once, chats first", () => {
    expect(serviceOptions().map((option) => option.value)).toEqual(ACCOUNT_KINDS);
    expect(ACCOUNT_KINDS.slice(0, CHAT_KINDS.length)).toEqual(CHAT_KINDS);
  });

  it("chat kinds include matrix", () => {
    expect(CHAT_KINDS).toContain("matrix");
  });

  it("new matrix account uses a token login", () => {
    expect(newAccount("matrix")).toMatchObject({ service: "matrix", login: "token" });
  });

  it("matrix fields follow the login", () => {
    expect(shown("token")).toEqual(["homeserver_url", "login", "access_token", "nickname"]);
    expect(shown("password")).toEqual([
      "homeserver_url",
      "login",
      "user",
      "password",
      "nickname",
      "recovery_key",
    ]);
    expect(shown("oauth")).toEqual(["homeserver_url", "login", "user", "nickname", "recovery_key"]);
  });

  it("lists the sign in of a device login", () => {
    const base = { ...MATRIX, login: "oauth" } as Account;
    expect(describeAccount(base)).toBe("Matrix · https://matrix.example · not signed in");
    expect(describeAccount({ ...base, session: { secret: "x" } } as Account)).toBe(
      "Matrix · https://matrix.example · signed in",
    );
  });

  it("counts a credential kept by id or typed in place as set", () => {
    expect(secretSet(undefined)).toBe(false);
    expect(secretSet(" ")).toBe(false);
    expect(secretSet("tok")).toBe(true);
    expect(secretSet({ secret: "a1" })).toBe(true);
    expect(secretSet({ secret: "" })).toBe(false);
  });
});
