import { CircleAlert } from "lucide-react";
import { newestRefusal, type Refusal, useRefusalStore } from "../lib/refusals";
import { Button } from "./BaseControls";
import { ICON_BTN_SM } from "./controls";
import { Icon } from "./Icon";

const NO_REFUSALS: readonly Refusal[] = [];

export function FaceAlert({ node }: { node: string }) {
  const list = useRefusalStore((store) => store.byNode[node] ?? NO_REFUSALS);
  const dismiss = useRefusalStore((store) => store.dismiss);
  const shown = newestRefusal(list);
  if (shown === null) {
    return null;
  }
  return (
    <Button
      type="button"
      role="alert"
      aria-label={shown.reason}
      title={`${shown.reason}\nClick to dismiss`}
      className={`${ICON_BTN_SM} shrink-0 text-danger`}
      onClick={() => dismiss(node)}
    >
      <Icon glyph={CircleAlert} size={12} />
    </Button>
  );
}
