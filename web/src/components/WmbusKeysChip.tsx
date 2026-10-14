import { Plus, X } from "lucide-react";
import { useState } from "react";
import { Button } from "./BaseControls";
import type { ChannelParamsOf } from "./channelSettings";
import { BTN_SM } from "./controls";
import { SettingChip } from "./face/Chips";
import { Icon } from "./Icon";
import { TextField } from "./TextField";

export type WmbusKey = NonNullable<ChannelParamsOf<"wmbus">["keys"]>[number];

const KEYS_TITLE = "AES keys per meter ID; frames from other meters stay locked";
const BLANK: WmbusKey = { meter: "", key: "" };

export function wmbusKeysShown(keys: readonly WmbusKey[]): string {
  return keys.length === 0 ? "none" : String(keys.length);
}

export function wmbusKeyComplete(entry: WmbusKey): boolean {
  return /^[0-9a-fA-F]{8}$/.test(entry.meter) && /^[0-9a-fA-F]{32}$/.test(entry.key);
}

export function WmbusKeysChip({
  keys,
  onChange,
}: {
  keys: readonly WmbusKey[];
  onChange: (keys: WmbusKey[]) => void;
}) {
  return (
    <SettingChip
      label="Keys"
      value={wmbusKeysShown(keys)}
      quiet={keys.length === 0}
      title={KEYS_TITLE}
      width="w-96"
    >
      {() => <WmbusKeysEditor keys={keys} onChange={onChange} />}
    </SettingChip>
  );
}

function WmbusKeysEditor({
  keys,
  onChange,
}: {
  keys: readonly WmbusKey[];
  onChange: (keys: WmbusKey[]) => void;
}) {
  const [draft, setDraft] = useState<WmbusKey | null>(null);
  const editDraft = (next: WmbusKey) => {
    if (wmbusKeyComplete(next)) {
      onChange([...keys, next]);
      setDraft(null);
    } else {
      setDraft(next);
    }
  };
  return (
    <div className="flex flex-col gap-2">
      {keys.map((entry, index) => (
        <WmbusKeyRow
          key={index}
          entry={entry}
          name={`Key ${index + 1}`}
          onEdit={(next) => onChange(keys.with(index, next))}
          onRemove={() => onChange(keys.filter((_, at) => at !== index))}
        />
      ))}
      {draft !== null && (
        <WmbusKeyRow
          entry={draft}
          name="New key"
          onEdit={editDraft}
          onRemove={() => setDraft(null)}
        />
      )}
      <Button
        type="button"
        className={`${BTN_SM} self-start`}
        disabled={draft !== null}
        onClick={() => setDraft(BLANK)}
      >
        <Icon glyph={Plus} size={12} />
        Key
      </Button>
    </div>
  );
}

function WmbusKeyRow({
  entry,
  name,
  onEdit,
  onRemove,
}: {
  entry: WmbusKey;
  name: string;
  onEdit: (next: WmbusKey) => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center gap-1">
      <TextField
        label={`${name} meter ID`}
        className="w-24 shrink-0"
        value={entry.meter}
        placeholder="Meter ID"
        maxLength={8}
        onCommit={(meter) => onEdit({ ...entry, meter })}
      />
      <TextField
        label={`${name} AES key`}
        className="min-w-0 flex-1"
        value={entry.key}
        secret
        placeholder="AES key"
        maxLength={32}
        onCommit={(key) => onEdit({ ...entry, key })}
      />
      <Button
        type="button"
        className={`${BTN_SM} hover:text-danger`}
        aria-label={`Remove ${name.toLowerCase()}`}
        onClick={onRemove}
      >
        <Icon glyph={X} size={12} />
      </Button>
    </div>
  );
}
