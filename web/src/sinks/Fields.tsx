import type { Field } from "../lib/types";
import { formatValue } from "./values";

export function keyedFields(fields: readonly Field[]): [string, Field][] {
  const seen = new Map<string, number>();
  return fields.map((field) => {
    const count = (seen.get(field.label) ?? 0) + 1;
    seen.set(field.label, count);
    return [count === 1 ? field.label : `${field.label}#${count}`, field];
  });
}

export function Fields({ fields }: { fields: readonly Field[] }) {
  if (fields.length === 0) {
    return null;
  }
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
      {keyedFields(fields).map(([key, field]) => (
        <div key={key} className="contents">
          <dt className="text-ink-dim">{field.label}</dt>
          <dd className="min-w-0 break-words font-mono tabular-nums text-ink">
            {formatValue(field.value)}
          </dd>
        </div>
      ))}
    </dl>
  );
}
