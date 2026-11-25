import { Button } from "../../components/BaseControls";
import { BTN_SM, KEY_SM } from "../../components/controls";
import { SettingChip } from "../../components/face/Chips";
import {
  activePreset,
  choiceSummary,
  enabledKinds,
  type ProtocolChoice,
  type ProtocolGroup,
  protocolPresets,
  setEnabled,
} from "./protocols";

export function ProtocolChip({
  groups,
  choice,
  onChange,
}: {
  groups: readonly ProtocolGroup[];
  choice: ProtocolChoice;
  onChange: (choice: ProtocolChoice) => void;
}) {
  const summary = choiceSummary(groups, choice);
  return (
    <SettingChip
      label="Protocols"
      value={summary}
      quiet={summary === "All protocols"}
      title="Protocols to decode"
      width="w-84"
      padded={false}
    >
      {() => <Panel groups={groups} choice={choice} onChange={onChange} />}
    </SettingChip>
  );
}

function Panel({
  groups,
  choice,
  onChange,
}: {
  groups: readonly ProtocolGroup[];
  choice: ProtocolChoice;
  onChange: (choice: ProtocolChoice) => void;
}) {
  const setDisabled = (kinds: readonly string[], enabled: boolean) =>
    onChange({ ...choice, disabled: setEnabled(choice.disabled, kinds, enabled) });
  return (
    <div className="flex flex-col">
      <Presets groups={groups} choice={choice} onChange={onChange} />
      <ul className="flex flex-col divide-y divide-line">
        {groups.map((group) => (
          <Family
            key={group.family}
            group={group}
            disabled={choice.disabled}
            onToggle={setDisabled}
          />
        ))}
      </ul>
      <div className="flex border-t border-line px-3 py-2">
        <Key
          label="Unidentified signals"
          title="Report signals no decoder recognises"
          on={choice.unidentified}
          onToggle={(unidentified) => onChange({ ...choice, unidentified })}
        />
      </div>
    </div>
  );
}

function Presets({
  groups,
  choice,
  onChange,
}: {
  groups: readonly ProtocolGroup[];
  choice: ProtocolChoice;
  onChange: (choice: ProtocolChoice) => void;
}) {
  const active = activePreset(groups, choice)?.id;
  return (
    <div
      role="group"
      aria-label="Presets"
      className="flex flex-wrap gap-1 border-b border-line p-2"
    >
      {protocolPresets(groups).map((preset) => (
        <Button
          key={preset.id}
          type="button"
          aria-pressed={preset.id === active}
          className={`${BTN_SM} aria-pressed:border-accent aria-pressed:bg-accent aria-pressed:text-on-accent`}
          onClick={() => onChange(preset.choice)}
        >
          {preset.label}
        </Button>
      ))}
    </div>
  );
}

function Family({
  group,
  disabled,
  onToggle,
}: {
  group: ProtocolGroup;
  disabled: readonly string[];
  onToggle: (kinds: readonly string[], enabled: boolean) => void;
}) {
  const kinds = group.protocols.map((protocol) => protocol.kind);
  const on = enabledKinds([group], disabled).length;
  return (
    <li className="flex flex-col gap-1.5 px-3 py-2">
      <div className="flex items-center gap-2 text-xs text-ink">
        <span className="flex-1">{group.title}</span>
        <span className="font-mono text-2xs tabular-nums text-ink-faint">
          {on}/{kinds.length}
        </span>
        <Key
          label="All"
          title={`Every ${group.title} protocol`}
          on={on === kinds.length}
          onToggle={(enabled) => onToggle(kinds, enabled)}
        />
      </div>
      <div className="flex flex-wrap gap-1">
        {group.protocols.map((protocol) => (
          <Key
            key={protocol.kind}
            label={protocol.label}
            title={protocol.name}
            on={!disabled.includes(protocol.kind)}
            onToggle={(enabled) => onToggle([protocol.kind], enabled)}
          />
        ))}
      </div>
    </li>
  );
}

function Key({
  label,
  title,
  on,
  onToggle,
}: {
  label: string;
  title: string;
  on: boolean;
  onToggle: (enabled: boolean) => void;
}) {
  return (
    <Button
      type="button"
      aria-pressed={on}
      title={title}
      className={KEY_SM}
      onClick={() => onToggle(!on)}
    >
      <span
        aria-hidden
        className={`size-1.5 rounded-full ${
          on ? "bg-accent shadow-glow shadow-accent" : "bg-line-strong"
        }`}
      />
      {label}
    </Button>
  );
}
