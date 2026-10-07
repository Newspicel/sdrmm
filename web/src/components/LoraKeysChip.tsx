import { Plus, X } from "lucide-react";
import { useState } from "react";
import { Button } from "./BaseControls";
import { BTN_SM, LABEL } from "./controls";
import { SettingChip } from "./face/Chips";
import { Icon } from "./Icon";
import {
  blankLoraKey,
  LORA_KEY_FIELDS,
  LORA_KEY_KINDS,
  type LoraKey,
  loraKeyComplete,
  loraKeyText,
  withLoraKeyText,
} from "./modeOptions";
import { TextField } from "./TextField";

const KEYS_TITLE = "Extra keys; Meshtastic defaults and MeshCore Public are built in";

export function loraKeysShown(keys: readonly LoraKey[]): string {
  return keys.length === 0 ? "built-in" : `+${keys.length}`;
}

export function LoraKeysChip({
  keys,
  onChange,
}: {
  keys: readonly LoraKey[];
  onChange: (keys: LoraKey[]) => void;
}) {
  return (
    <SettingChip
      label="Keys"
      value={loraKeysShown(keys)}
      quiet={keys.length === 0}
      title={KEYS_TITLE}
      width="w-96"
    >
      {() => <LoraKeysEditor keys={keys} onChange={onChange} />}
    </SettingChip>
  );
}

function LoraKeysEditor({
  keys,
  onChange,
}: {
  keys: readonly LoraKey[];
  onChange: (keys: LoraKey[]) => void;
}) {
  const [draft, setDraft] = useState<LoraKey | null>(null);
  const editDraft = (next: LoraKey) => {
    if (loraKeyComplete(next)) {
      onChange([...keys, next]);
      setDraft(null);
    } else {
      setDraft(next);
    }
  };
  return (
    <div className="flex flex-col gap-2">
      {keys.map((entry, index) => (
        <LoraKeyRow
          key={index}
          entry={entry}
          name={`Key ${index + 1}`}
          onEdit={(next) => onChange(keys.with(index, next))}
          onRemove={() => onChange(keys.filter((_, at) => at !== index))}
        />
      ))}
      {draft !== null && (
        <LoraKeyRow
          entry={draft}
          name="New key"
          onEdit={editDraft}
          onRemove={() => setDraft(null)}
        />
      )}
      <div className="flex flex-wrap items-center gap-1">
        {LORA_KEY_KINDS.map((kind) => (
          <Button
            key={kind.value}
            type="button"
            className={BTN_SM}
            title={kind.title}
            onClick={() => setDraft(blankLoraKey(kind.value))}
          >
            <Icon glyph={Plus} size={12} />
            {kind.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

function LoraKeyRow({
  entry,
  name,
  onEdit,
  onRemove,
}: {
  entry: LoraKey;
  name: string;
  onEdit: (next: LoraKey) => void;
  onRemove: () => void;
}) {
  const kind = LORA_KEY_KINDS.find((option) => option.value === entry.kind);
  return (
    <div className="flex items-center gap-1">
      <span className={`${LABEL} w-16 shrink-0`} title={kind?.title}>
        {kind?.label}
      </span>
      <div className="flex min-w-0 flex-1 flex-wrap gap-1">
        {LORA_KEY_FIELDS[entry.kind].map(([field, placeholder]) => (
          <TextField
            key={field}
            label={`${name} ${placeholder}`}
            className="min-w-0 flex-1 basis-24"
            value={loraKeyText(entry, field)}
            placeholder={placeholder}
            onCommit={(text) => onEdit(withLoraKeyText(entry, field, text))}
          />
        ))}
      </div>
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
