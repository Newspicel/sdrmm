import { Toggle } from "@base-ui/react/toggle";
import { ToggleGroup } from "@base-ui/react/toggle-group";
import { type Options, segment, WELL } from "./controls";

export function Segmented<T extends string | number>({
  label,
  value,
  options,
  onChange,
  fill = false,
}: {
  label: string;
  value: T;
  options: Options<T>;
  onChange: (value: T) => void;
  fill?: boolean;
}) {
  return (
    <ToggleGroup
      data-hotkeys="off"
      aria-label={label}
      className={`${WELL} ${fill ? "w-full" : "w-fit"}`}
      value={[String(value)]}
      onValueChange={(next) => {
        const picked = options.find((option) => String(option.value) === next[0]);
        if (picked !== undefined && picked.disabled !== true) {
          onChange(picked.value);
        }
      }}
    >
      {options.map((option) => {
        const toggle = (
          <Toggle
            key={String(option.value)}
            value={String(option.value)}
            title={option.title}
            disabled={option.disabled}
            className={(state) =>
              `${segment(state.pressed)} tabular-nums ${
                fill ? "flex-auto justify-center whitespace-nowrap" : ""
              }`
            }
          >
            {option.label}
          </Toggle>
        );
        return option.disabled === true && option.title !== undefined ? (
          <span
            key={String(option.value)}
            title={option.title}
            className={`inline-flex ${fill ? "flex-auto" : ""}`}
          >
            {toggle}
          </span>
        ) : (
          toggle
        );
      })}
    </ToggleGroup>
  );
}

export function SegmentedToggles<T extends string | number>({
  label,
  values,
  options,
  onChange,
}: {
  label: string;
  values: readonly T[];
  options: Options<T>;
  onChange: (values: T[]) => void;
}) {
  return (
    <ToggleGroup
      multiple
      data-hotkeys="off"
      aria-label={label}
      className={`${WELL} w-fit`}
      value={values.map(String)}
      onValueChange={(next) => {
        const picked = keptOn(options, next);
        if (picked.length > 0) {
          onChange(picked);
        }
      }}
    >
      {options.map((option) => {
        const last = isLastOn(values, option.value);
        return (
          <Toggle
            key={String(option.value)}
            value={String(option.value)}
            title={last ? "Last one stays on" : option.title}
            disabled={option.disabled === true || last}
            className={(state) =>
              `${segment(state.pressed)} tabular-nums ${last ? "cursor-default" : ""}`
            }
          >
            {option.label}
          </Toggle>
        );
      })}
    </ToggleGroup>
  );
}

export function keptOn<T extends string | number>(
  options: Options<T>,
  pressed: readonly unknown[],
): T[] {
  return options
    .filter((option) => option.disabled !== true && pressed.includes(String(option.value)))
    .map((option) => option.value);
}

export function isLastOn<T extends string | number>(values: readonly T[], value: T): boolean {
  return values.length === 1 && values[0] === value;
}
