import { CheckboxIndicator, Checkbox as Primitive } from "@ui-sdk/react/checkbox";
import { Check } from "lucide-react";
import { useRows, YesNo } from "./face/Chips";
import { Icon } from "./Icon";

export function Checkbox({
  label,
  checked,
  disabled,
  onChange,
}: {
  label?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  if (useRows()) {
    return <YesNo label={label ?? ""} on={checked} disabled={disabled} onChange={onChange} />;
  }
  return (
    <Primitive
      aria-label={label}
      checked={checked}
      disabled={disabled}
      onCheckedChange={(next) => onChange(next === true)}
      className="relative flex size-4 shrink-0 items-center justify-center rounded-control border border-line-strong bg-well text-on-accent data-checked:border-accent data-checked:bg-accent data-disabled:opacity-45 touch:before:absolute touch:before:-inset-3"
    >
      <CheckboxIndicator className="flex data-unchecked:hidden">
        <Icon glyph={Check} size={12} />
      </CheckboxIndicator>
    </Primitive>
  );
}
