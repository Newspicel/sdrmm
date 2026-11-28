import type { InvalidateQueryFilters, QueryClient } from "@tanstack/react-query";

interface Round {
  again: boolean;
}

const rounds = new WeakMap<QueryClient, Map<string, Round>>();

/** Refetches what `filters` match, one round at a time per `id`. A hint that lands while a round
 * runs asks for one more round after it, so a burst of hints costs two fetches, not one each
 * cancelling the last, and the data still ends newer than the last hint. */
export function refresh(
  queryClient: QueryClient,
  id: string,
  filters: InvalidateQueryFilters,
): void {
  const held = rounds.get(queryClient) ?? new Map<string, Round>();
  rounds.set(queryClient, held);
  const running = held.get(id);
  if (running !== undefined) {
    running.again = true;
    return;
  }
  const round: Round = { again: false };
  held.set(id, round);
  void (async () => {
    try {
      do {
        round.again = false;
        await queryClient.invalidateQueries(filters);
      } while (round.again);
    } finally {
      held.delete(id);
    }
  })();
}

/** `refresh` for one query key. */
export function refreshKey(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  exact = false,
): void {
  refresh(queryClient, `${JSON.stringify(queryKey)}${exact ? "=" : ""}`, { queryKey, exact });
}
