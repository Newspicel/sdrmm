import { linkTrouble, usePipelineHealth } from "../lib/pipeline";

export function LinkQuality() {
  const trouble = linkTrouble(usePipelineHealth((state) => state.health));
  if (trouble === null) {
    return null;
  }
  return (
    <span
      role="status"
      title={trouble.detail}
      className="px-1 font-mono text-2xs text-warn tabular-nums"
    >
      Link {trouble.label}
    </span>
  );
}
