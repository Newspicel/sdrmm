import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { PHONES_KEY, STATE_KEY, WORKSPACES_KEY } from "./api";
import { invalidateScope } from "./useSdrSocket";

function seeded(): QueryClient {
  const client = new QueryClient();
  client.setQueryData(PHONES_KEY, { phones: [] });
  client.setQueryData(STATE_KEY, { device_sets: [] });
  return client;
}

describe("invalidateScope", () => {
  it("refreshes the phone list when phones change", () => {
    const client = seeded();
    invalidateScope(client, { scope: "phones" });
    expect(client.getQueryState(PHONES_KEY)?.isInvalidated).toBe(true);
    expect(client.getQueryState(STATE_KEY)?.isInvalidated).toBe(false);
  });

  it("leaves every query alone when missions change", () => {
    const client = seeded();
    invalidateScope(client, { scope: "missions" });
    expect(client.getQueryState(PHONES_KEY)?.isInvalidated).toBe(false);
    expect(client.getQueryState(STATE_KEY)?.isInvalidated).toBe(false);
  });

  it("refreshes only the one workspace a dial step touched", () => {
    const client = seeded();
    client.setQueryData(WORKSPACES_KEY, { workspaces: [] });
    client.setQueryData([...WORKSPACES_KEY, 1], {});
    client.setQueryData([...WORKSPACES_KEY, 2], {});
    invalidateScope(client, { scope: "workspace", id: 1 });
    expect(client.getQueryState([...WORKSPACES_KEY, 1])?.isInvalidated).toBe(true);
    expect(client.getQueryState([...WORKSPACES_KEY, 2])?.isInvalidated).toBe(false);
    expect(client.getQueryState(WORKSPACES_KEY)?.isInvalidated).toBe(false);
  });
});
