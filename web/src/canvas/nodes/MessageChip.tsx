import { useRef, useState } from "react";
import { Button, Textarea } from "../../components/BaseControls";
import { CHIP_SETTING_QUIET, FIELD_AREA } from "../../components/controls";
import { ChipField, SettingChip } from "../../components/face/Chips";
import { MESSAGE_LIMITS } from "../../lib/limits";
import { insertAt, messageShown, placeholderToken } from "./eventOutput";

const TITLE = "Your own message layout. Empty sends the usual one";

export function MessageChip({
  value,
  onCommit,
}: {
  value: string;
  onCommit: (value: string) => void;
}) {
  return (
    <SettingChip
      label="Message"
      value={messageShown(value)}
      title={TITLE}
      quiet={value.trim() === ""}
      width="w-80"
    >
      {() => (
        <ChipField label="Message layout">
          <MessageEditor value={value} onCommit={onCommit} />
        </ChipField>
      )}
    </SettingChip>
  );
}

function MessageEditor({ value, onCommit }: { value: string; onCommit: (value: string) => void }) {
  const area = useRef<HTMLTextAreaElement>(null);
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
  const place = (name: string) => {
    const field = area.current;
    const start = field?.selectionStart ?? draft.length;
    const end = field?.selectionEnd ?? draft.length;
    const next = insertAt(draft, start, end, placeholderToken(name));
    setDraft(next.text);
    requestAnimationFrame(() => {
      field?.focus();
      field?.setSelectionRange(next.caret, next.caret);
    });
  };
  return (
    <div className="flex w-full min-w-0 flex-col gap-2">
      <Textarea
        ref={area}
        className={`${FIELD_AREA} nodrag nopan min-h-20 w-full resize-y`}
        aria-label="Message layout"
        placeholder="**{headline}** on {freq} MHz"
        maxLength={MESSAGE_LIMITS.len}
        rows={4}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => event.stopPropagation()}
      />
      <div className="flex flex-wrap gap-1" role="group" aria-label="Placeholders">
        {MESSAGE_LIMITS.placeholders.map((name) => (
          <Button
            key={name}
            type="button"
            className={`${CHIP_SETTING_QUIET} font-mono`}
            title={name.endsWith(".") ? "A field of the decoder's record, by path" : undefined}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => place(name)}
          >
            {name.endsWith(".") ? `{${name}…}` : placeholderToken(name)}
          </Button>
        ))}
      </div>
    </div>
  );
}
