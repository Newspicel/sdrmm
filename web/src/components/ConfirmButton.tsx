import type { LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "./BaseControls";
import {
  BTN_DANGER,
  BTN_DANGER_ARMED,
  BTN_DANGER_SM,
  BTN_SM_DANGER,
  ICON_BTN_SM_DANGER,
  ICON_BTN_SM_DANGER_ARMED,
} from "./controls";
import { Icon } from "./Icon";

export const CONFIRM_ARM_MS = 3000;

export type ConfirmSize = "md" | "sm" | "icon";

const CLASS: Record<ConfirmSize, readonly [idle: string, armed: string]> = {
  md: [BTN_DANGER, BTN_DANGER_ARMED],
  sm: [BTN_SM_DANGER, BTN_DANGER_SM],
  icon: [ICON_BTN_SM_DANGER, ICON_BTN_SM_DANGER_ARMED],
};

export function confirmClass(size: ConfirmSize, armed: boolean): string {
  return CLASS[size][armed ? 1 : 0];
}

export function confirmLabel(label: string): string {
  return `Confirm ${label.charAt(0).toLowerCase()}${label.slice(1)}`;
}

export function ConfirmButton({
  label,
  onConfirm,
  title,
  disabled = false,
  size = "md",
  icon,
}: {
  label: string;
  onConfirm: () => void;
  title?: string;
  disabled?: boolean;
  size?: ConfirmSize;
  icon?: LucideIcon;
}) {
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (!armed) {
      return;
    }
    const timer = window.setTimeout(() => setArmed(false), CONFIRM_ARM_MS);
    return () => window.clearTimeout(timer);
  }, [armed]);

  const text = armed ? confirmLabel(label) : label;
  return (
    <Button
      type="button"
      className={confirmClass(size, armed)}
      disabled={disabled}
      title={armed ? "Click again to confirm" : (title ?? (icon === undefined ? undefined : label))}
      aria-label={icon === undefined ? undefined : text}
      onBlur={() => setArmed(false)}
      onClick={() => {
        if (armed) {
          onConfirm();
        }
        setArmed(!armed);
      }}
    >
      {icon === undefined ? text : <Icon glyph={icon} size={12} />}
    </Button>
  );
}
