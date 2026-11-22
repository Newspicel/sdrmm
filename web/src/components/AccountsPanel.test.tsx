import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { ChatAccount } from "../lib/types";
import { type AccountActions, AccountsView } from "./AccountsPanel";

const DISCORD: ChatAccount = {
  id: "a1",
  name: "Club bot",
  service: "discord",
  token: { secret: "secret-token-id" },
};

const TEAMSPEAK: ChatAccount = {
  id: "a2",
  name: "Net",
  service: "teamspeak",
  address: "ts.example:9987",
  nickname: "sdr",
  password: { secret: "secret-password-id" },
  identity: { secret: "secret-identity-id" },
};

function actions(): AccountActions {
  return { draft: vi.fn(), save: vi.fn(), remove: vi.fn() };
}

function view(accounts: ChatAccount[] | undefined, draft: ChatAccount | null = null): string {
  return renderToStaticMarkup(
    <AccountsView accounts={accounts} failed={false} draft={draft} busy={false} on={actions()} />,
  );
}

describe("AccountsView", () => {
  it("lists saved accounts without their secrets", () => {
    const html = view([DISCORD, TEAMSPEAK]);
    expect(html).toContain("Club bot");
    expect(html).toContain("Discord");
    expect(html).toContain("ts.example:9987");
    expect(html).not.toContain("secret-");
  });

  it("edits an account with its stored secrets shown only as set", () => {
    const html = view([DISCORD, TEAMSPEAK], TEAMSPEAK);
    expect(html).toContain('aria-label="TeamSpeak server password, kept on this server"');
    expect(html).toContain('placeholder="set"');
    expect(html).not.toContain("secret-");
  });

  it("offers Clear only for a TeamSpeak password that is set", () => {
    expect(view([TEAMSPEAK], TEAMSPEAK)).toContain('aria-label="Clear password"');
    expect(view([TEAMSPEAK], { ...TEAMSPEAK, password: "" })).not.toContain("Clear password");
    expect(view([DISCORD], DISCORD)).not.toContain("Clear token");
  });

  it("locks every other edit and add while a form is open", () => {
    const html = view([DISCORD, TEAMSPEAK], TEAMSPEAK);
    expect(html).toMatch(/<button[^>]*aria-label="Edit Club bot"[^>]*disabled=""/);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Add Discord</);
  });

  it("offers both services to add and says when there are none", () => {
    const html = view([]);
    expect(html).toContain("Add Discord");
    expect(html).toContain("Add TeamSpeak");
    expect(html).toContain("No accounts");
  });
});
