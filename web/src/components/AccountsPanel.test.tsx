import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Account } from "../lib/types";
import { type AccountActions, AccountsView } from "./AccountsPanel";

const DISCORD: Account = {
  id: "a1",
  name: "Club bot",
  service: "discord",
  token: { secret: "secret-token-id" },
};

const TEAMSPEAK: Account = {
  id: "a2",
  name: "Net",
  service: "teamspeak",
  address: "ts.example:9987",
  nickname: "sdr",
  password: { secret: "secret-password-id" },
  identity: { secret: "secret-identity-id" },
};

const BROKER: Account = {
  id: "a3",
  name: "Broker",
  service: "mqtt",
  broker_url: "mqtts://broker.example",
  username: "radio",
  password: { secret: "secret-broker-id" },
};

const HOOK: Account = {
  id: "a4",
  name: "Alerts",
  service: "webhook",
  url: { secret: "secret-url" },
};

function actions(): AccountActions {
  return { draft: vi.fn(), save: vi.fn(), remove: vi.fn() };
}

function view(accounts: Account[] | undefined, draft: Account | null = null): string {
  return renderToStaticMarkup(
    <AccountsView accounts={accounts} failed={false} draft={draft} busy={false} on={actions()} />,
  );
}

describe("AccountsView", () => {
  it("lists saved accounts of every service without their secrets", () => {
    const html = view([DISCORD, TEAMSPEAK, BROKER, HOOK]);
    expect(html).toContain("Club bot");
    expect(html).toContain("Discord");
    expect(html).toContain("ts.example:9987");
    expect(html).toContain("MQTT · mqtts://broker.example");
    expect(html).toContain("Alerts");
    expect(html).not.toContain("secret-");
  });

  it("edits an account with its stored secrets shown only as set", () => {
    const html = view([DISCORD, TEAMSPEAK], TEAMSPEAK);
    expect(html).toContain('aria-label="TeamSpeak server password, kept on this server"');
    expect(html).toContain('placeholder="set"');
    expect(html).not.toContain("secret-");
  });

  it("builds each form from the service's fields", () => {
    const html = view([BROKER], BROKER);
    expect(html).toContain('aria-label="MQTT broker URL"');
    expect(html).toContain('aria-label="MQTT username"');
    expect(html).toContain('aria-label="MQTT password, kept on this server"');
    expect(view([HOOK], HOOK)).toContain('aria-label="Webhook URL, kept on this server"');
  });

  it("offers Clear only for an optional credential that is set", () => {
    expect(view([TEAMSPEAK], TEAMSPEAK)).toContain('aria-label="Clear password"');
    expect(view([BROKER], BROKER)).toContain('aria-label="Clear password"');
    expect(view([TEAMSPEAK], { ...TEAMSPEAK, password: "" })).not.toContain("Clear password");
    expect(view([DISCORD], DISCORD)).not.toContain("Clear token");
    expect(view([HOOK], HOOK)).not.toContain("Clear endpoint");
  });

  it("locks every other edit and the add choice while a form is open", () => {
    const html = view([DISCORD, TEAMSPEAK], TEAMSPEAK);
    expect(html).toMatch(/<button[^>]*aria-label="Edit Club bot"[^>]*disabled=""/);
    expect(html).toMatch(
      /aria-label="Add account"[^>]*disabled=""|disabled=""[^>]*aria-label="Add account"/,
    );
  });

  it("adds through one choice and says when there are none", () => {
    const html = view([]);
    expect(html).toContain('aria-label="Add account"');
    expect(html).not.toContain("Add Discord");
    expect(html).toContain("No accounts");
  });
});
