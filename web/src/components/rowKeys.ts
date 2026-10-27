export function rowKeys(rows: readonly unknown[]): string[] {
  const repeats = new Map<string, number>();
  return rows.map((row) => {
    const content = JSON.stringify(row) ?? "";
    const seen = repeats.get(content) ?? 0;
    repeats.set(content, seen + 1);
    return `${content}#${seen}`;
  });
}
