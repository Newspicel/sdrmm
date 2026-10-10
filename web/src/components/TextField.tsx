import { useState } from "react";
import { Input } from "./BaseControls";
import { CONTROL_W, FIELD } from "./controls";

export function TextField({
  label,
  value,
  secret = false,
  placeholder,
  maxLength,
  className = CONTROL_W,
  onCommit,
}: {
  label: string;
  value: string;
  secret?: boolean;
  placeholder?: string;
  maxLength?: number;
  className?: string;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const [shown, setShown] = useState(value);
  if (shown !== value) {
    setShown(value);
    setDraft(value);
  }
  const commit = () => {
    const next = draft.trim();
    setDraft(next);
    if (next !== value) {
      onCommit(next);
    }
  };
  return (
    <Input
      className={`${FIELD} ${className}`}
      aria-label={label}
      type={secret ? "password" : "text"}
      autoComplete="off"
      placeholder={placeholder}
      maxLength={maxLength}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.currentTarget.blur();
        }
      }}
    />
  );
}
