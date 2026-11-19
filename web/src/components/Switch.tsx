import { Switch as Primitive, SwitchThumb } from "@ui-sdk/react/switch";

export function Switch({
  label,
  checked,
  disabled = false,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <Primitive
      aria-label={label}
      checked={checked}
      disabled={disabled}
      onCheckedChange={onChange}
      className="relative flex h-4 w-7 shrink-0 cursor-pointer items-center rounded-full border border-line bg-well p-px transition-colors duration-150 data-checked:border-accent data-checked:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent pointer-coarse:before:absolute pointer-coarse:before:-inset-3"
    >
      <SwitchThumb className="size-3 rounded-full bg-ink-faint shadow-raised transition-transform duration-150 data-checked:translate-x-3 data-checked:bg-on-accent" />
    </Primitive>
  );
}
