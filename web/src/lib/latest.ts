export function latestOnly<T>(work: (item: T) => Promise<void>): (item: T) => void {
  let busy = false;
  let waiting: { item: T } | null = null;
  const run = (item: T): void => {
    busy = true;
    void work(item)
      .catch(() => {})
      .then(() => {
        busy = false;
        const next = waiting;
        waiting = null;
        if (next !== null) {
          run(next.item);
        }
      });
  };
  return (item) => {
    if (busy) {
      waiting = { item };
    } else {
      run(item);
    }
  };
}
