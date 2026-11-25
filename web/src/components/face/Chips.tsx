import { createContext, type ReactNode, useContext } from "react";
import { Button } from "../BaseControls";
import {
  CHIP_READOUT,
  CHIP_SETTING,
  CHIP_SETTING_QUIET,
  listItem,
  type Options,
  WRAPPED_LABEL,
} from "../controls";
import { NumberField, OptionalNumberField } from "../NumberField";
import { Popover } from "../Popover";
import { Select } from "../Select";
import { Tip } from "../Tip";

export type ChipTone = "ok" | "warn" | "danger";

const TONE: Record<ChipTone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  danger: "text-danger",
};

/** How chips draw: compact on a face, or one labelled row each where there is room. */
export type ChipLayout = "chip" | "row";

const Layout = createContext<ChipLayout>("chip");

export function ChipRows({ children }: { children: ReactNode }) {
  return <Layout value="row">{children}</Layout>;
}

/** True inside the inspector, where every setting is a labelled row. */
export function useRows(): boolean {
  return useContext(Layout) === "row";
}

const YES_NO = [
  { value: "yes", label: "Yes" },
  { value: "no", label: "No" },
] as const;

/** An on/off setting as the inspector draws it: the same dropdown every choice uses. */
export function YesNo({
  label,
  on,
  disabled,
  onChange,
}: {
  label: string;
  on: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <Select
      label={label}
      value={on ? "yes" : "no"}
      options={YES_NO}
      disabled={disabled}
      className="w-full"
      onChange={(next) => onChange(next === "yes")}
    />
  );
}

const ROWS = "flex flex-col gap-5";

/** A setting with its name small above it. */
function ChipRow({
  label,
  title,
  children,
}: {
  label: string;
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-2xs text-ink-dim wrap-anywhere" title={title}>
        {label}
      </span>
      <div className="flex min-h-7 min-w-0 flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

/** A setting that fits on one line: its name, then the switch or value at the far edge. */
function InlineRow({
  label,
  title,
  children,
}: {
  label: string;
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-7 min-w-0 items-center justify-between gap-2">
      <span className="text-ink-dim text-xs wrap-anywhere" title={title}>
        {label}
      </span>
      {children}
    </div>
  );
}

function OptionList<T extends string | number>({
  value,
  options,
  title,
  disabled,
  onPick,
}: {
  value: T;
  options: Options<T>;
  title: string;
  disabled?: boolean;
  onPick: (value: T) => void;
}) {
  return (
    <div
      role="listbox"
      aria-label={title}
      className="flex max-h-56 w-full flex-col overflow-y-auto rounded-control border border-line bg-well py-1"
    >
      {options.map((option) => (
        <Button
          key={String(option.value)}
          type="button"
          role="option"
          aria-selected={option.value === value}
          title={option.title}
          disabled={disabled === true || option.disabled === true}
          className={listItem(option.value === value, false)}
          onClick={() => onPick(option.value)}
        >
          {option.label}
        </Button>
      ))}
    </div>
  );
}

export type ChipsInset = "full" | "below" | "none";

const INSET: Record<ChipsInset, string> = {
  full: "p-2",
  below: "px-2 pb-2",
  none: "",
};

export function Chips({
  children,
  className,
  inset = "full",
}: {
  children: ReactNode;
  className?: string;
  inset?: ChipsInset;
}) {
  if (useRows()) {
    return <div className={`${ROWS} ${className ?? ""}`}>{children}</div>;
  }
  return (
    <div
      className={`flex flex-wrap items-center gap-x-1 gap-y-0.75 contain-inline-size ${INSET[inset]} ${className ?? ""}`}
    >
      {children}
    </div>
  );
}

function ChipFace({
  label,
  value,
  unit,
  quiet,
  tone,
}: {
  label: string;
  value: string;
  unit?: string;
  quiet: boolean;
  tone?: ChipTone;
}) {
  const shade =
    tone !== undefined
      ? `font-medium ${TONE[tone]}`
      : quiet
        ? "font-normal text-ink-faint"
        : "font-medium text-ink";
  return (
    <>
      <span className="font-sans">{label}</span>
      <b className={shade}>{value}</b>
      {unit !== undefined && unit !== "" && <span>{unit}</span>}
    </>
  );
}

export function ChipField({ label, children }: { label: string; children: ReactNode }) {
  if (useRows()) {
    return (
      <div className="group/field flex min-w-0 flex-col gap-1">
        <span className="text-2xs text-ink-dim group-only/field:hidden">{label}</span>
        <span className="flex min-w-0 flex-wrap items-center gap-2 [&>div]:min-w-0 [&>div]:flex-1 [&>input]:min-w-0 [&>input]:flex-1">
          {children}
        </span>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      <span className={WRAPPED_LABEL}>{label}</span>
      <span className="flex min-w-0 flex-wrap items-center gap-2">{children}</span>
    </div>
  );
}

export function SettingChip({
  label,
  value,
  unit,
  title,
  quiet = false,
  tone,
  disabled = false,
  padded = true,
  width = "w-64",
  children,
}: {
  label: string;
  value: string;
  unit?: string;
  title: string;
  quiet?: boolean;
  tone?: ChipTone;
  disabled?: boolean;
  padded?: boolean;
  width?: string;
  children: (close: () => void) => ReactNode;
}) {
  if (useRows()) {
    return (
      <div className="flex min-w-0 flex-col gap-1 [&:has(>fieldset>:nth-child(2))>span]:hidden">
        <span className="text-2xs text-ink-dim wrap-anywhere" title={title}>
          {label}
        </span>
        <fieldset
          disabled={disabled}
          className="m-0 flex min-w-0 flex-col gap-2 border-0 p-0 disabled:opacity-45"
        >
          {children(stayOpen)}
        </fieldset>
      </div>
    );
  }
  return (
    <Popover
      label={<ChipFace label={label} value={value} unit={unit} quiet={quiet} tone={tone} />}
      title={title}
      triggerClass={quiet && tone === undefined ? CHIP_SETTING_QUIET : CHIP_SETTING}
      disabled={disabled}
      padded={padded}
      width={width}
    >
      {children}
    </Popover>
  );
}

function stayOpen(): void {}

export function ChoiceChip<T extends string | number>({
  label,
  value,
  options,
  title,
  quiet,
  tone,
  disabled,
  listed = false,
  onChange,
}: {
  label: string;
  value: T;
  options: Options<T>;
  title: string;
  quiet?: boolean;
  tone?: ChipTone;
  disabled?: boolean;
  /** In the inspector, every option in view at once: for what the running decoder found. */
  listed?: boolean;
  onChange: (value: T) => void;
}) {
  const rows = useRows();
  const current = options.find((option) => option.value === value);
  const pick = (next: T): void => {
    if (next !== value) {
      onChange(next);
    }
  };
  if (rows) {
    return (
      <ChipRow label={label} title={title}>
        {listed ? (
          <OptionList
            value={value}
            options={options}
            title={title}
            disabled={disabled}
            onPick={pick}
          />
        ) : (
          <Select
            label={title}
            value={value}
            options={options}
            disabled={disabled}
            className={`w-full ${tone === undefined ? "" : TONE[tone]}`}
            onChange={pick}
          />
        )}
      </ChipRow>
    );
  }
  return (
    <SettingChip
      label={label}
      value={current?.label ?? String(value)}
      title={title}
      quiet={quiet}
      tone={tone}
      disabled={disabled}
      padded={false}
      width="w-44"
    >
      {(close) => (
        <div role="listbox" aria-label={title} className="flex flex-col py-1">
          {options.map((option) => (
            <Button
              key={String(option.value)}
              type="button"
              role="option"
              aria-selected={option.value === value}
              title={option.title}
              disabled={option.disabled}
              className={listItem(option.value === value, false)}
              onClick={() => {
                if (option.value !== value) {
                  onChange(option.value);
                }
                close();
              }}
            >
              {option.label}
            </Button>
          ))}
        </div>
      )}
    </SettingChip>
  );
}

export function NumberChip({
  label,
  value,
  shown,
  unit,
  title,
  min,
  max,
  step,
  quiet,
  disabled,
  onCommit,
}: {
  label: string;
  value: number;
  shown?: string;
  unit?: string;
  title: string;
  min?: number;
  max?: number;
  step?: number;
  quiet?: boolean;
  disabled?: boolean;
  onCommit: (value: number) => void;
}) {
  return (
    <SettingChip
      label={label}
      value={shown ?? String(value)}
      unit={shown === undefined ? unit : undefined}
      title={title}
      quiet={quiet}
      disabled={disabled}
      width="w-52"
    >
      {() => (
        <ChipField label={title}>
          <NumberField
            className="min-w-0 flex-1"
            label={title}
            value={value}
            unit={unit}
            min={min}
            max={max}
            step={step}
            onCommit={onCommit}
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

export function OptionalNumberChip({
  label,
  value,
  placeholder,
  shown,
  unit,
  title,
  min,
  max,
  step,
  quiet = value === null,
  onCommit,
}: {
  label: string;
  value: number | null;
  placeholder: string;
  shown?: string;
  unit?: string;
  title: string;
  min?: number;
  max?: number;
  step?: number;
  quiet?: boolean;
  onCommit: (value: number | null) => void;
}) {
  return (
    <SettingChip
      label={label}
      value={value === null ? placeholder : (shown ?? String(value))}
      unit={value === null || shown !== undefined ? undefined : unit}
      title={title}
      quiet={quiet}
      width="w-52"
    >
      {() => (
        <ChipField label={title}>
          <OptionalNumberField
            className="min-w-0 flex-1"
            label={title}
            placeholder={placeholder}
            value={value}
            unit={unit}
            min={min}
            max={max}
            step={step}
            onCommit={onCommit}
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

export function ReadoutChip({
  label,
  value,
  unit,
  title,
  tone,
}: {
  label: string;
  value: string;
  unit?: string;
  title: string;
  tone?: ChipTone;
}) {
  if (useRows()) {
    return (
      <InlineRow label={label} title={title}>
        <span className={`font-mono text-xs ${tone === undefined ? "text-ink" : TONE[tone]}`}>
          {value}
          {unit !== undefined && unit !== "" && ` ${unit}`}
        </span>
      </InlineRow>
    );
  }
  return (
    <span className={CHIP_READOUT} title={title}>
      <ChipFace label={label} value={value} unit={unit} quiet={false} tone={tone} />
    </span>
  );
}

export function ToggleChip({
  label,
  on,
  title,
  icon,
  disabled = false,
  onChange,
}: {
  label: string;
  on: boolean;
  title: string;
  icon?: ReactNode;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  if (useRows()) {
    return (
      <ChipRow label={label} title={title}>
        <YesNo label={title} on={on} disabled={disabled} onChange={onChange} />
      </ChipRow>
    );
  }
  return (
    <Tip
      text={title}
      render={
        <Button
          type="button"
          className={on ? CHIP_SETTING : CHIP_SETTING_QUIET}
          aria-label={title}
          aria-pressed={on}
          disabled={disabled}
          onClick={() => onChange(!on)}
        />
      }
    >
      {icon}
      <span className="font-sans">{label}</span>
      <b className={on ? "font-medium text-accent" : "font-normal text-ink-faint"}>
        {on ? "on" : "off"}
      </b>
    </Tip>
  );
}
