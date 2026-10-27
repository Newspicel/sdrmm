export function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) {
    return true;
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    return Array.isArray(a) && Array.isArray(b) && sameItems(a, b);
  }
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    return false;
  }
  return sameFields(a as Record<string, unknown>, b as Record<string, unknown>);
}

function sameItems(a: readonly unknown[], b: readonly unknown[]): boolean {
  return a.length === b.length && a.every((item, index) => deepEqual(item, b[index]));
}

function sameFields(a: Record<string, unknown>, b: Record<string, unknown>): boolean {
  const keys = Object.keys(a).filter((key) => a[key] !== undefined);
  const others = Object.keys(b).filter((key) => b[key] !== undefined);
  return keys.length === others.length && keys.every((key) => deepEqual(a[key], b[key]));
}
