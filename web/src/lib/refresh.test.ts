import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { refreshKey } from "./refresh";

const KEY = ["get", "/api/state"] as const;

function watched(): { client: QueryClient; fetches: () => number; version: () => number } {
  const client = new QueryClient();
  let fetches = 0;
  let version = 0;
  const observer = new QueryObserver(client, {
    queryKey: KEY,
    queryFn: async () => {
      fetches += 1;
      const seen = version;
      await new Promise((resolve) => setTimeout(resolve, 5));
      return seen;
    },
  });
  observer.subscribe(() => {});
  return { client, fetches: () => fetches, version: () => (version += 1) };
}

async function settled(client: QueryClient): Promise<void> {
  while (client.isFetching() > 0) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}

describe("refreshKey", () => {
  it("folds a burst of hints into two fetches that end on the newest data", async () => {
    const { client, fetches, version } = watched();
    await settled(client);
    const before = fetches();
    let last = 0;
    for (let hint = 0; hint < 100; hint++) {
      last = version();
      refreshKey(client, KEY);
    }
    await settled(client);
    await new Promise((resolve) => setTimeout(resolve, 20));
    await settled(client);
    expect(fetches() - before).toBe(2);
    expect(client.getQueryData(KEY)).toBe(last);
  });

  it("fetches again for a hint that lands after the round", async () => {
    const { client, fetches, version } = watched();
    await settled(client);
    const before = fetches();
    version();
    refreshKey(client, KEY);
    await settled(client);
    await new Promise((resolve) => setTimeout(resolve, 1));
    const last = version();
    refreshKey(client, KEY);
    await settled(client);
    expect(fetches() - before).toBe(2);
    expect(client.getQueryData(KEY)).toBe(last);
  });
});
