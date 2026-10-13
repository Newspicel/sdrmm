import { nodeAction } from "../../lib/api";

export const ANALOG_MODES = ["nfm", "wfm", "am", "ssb"] as const;

export function nextAnalogMode(current: string, direction: number): string {
  const at = ANALOG_MODES.indexOf(current as (typeof ANALOG_MODES)[number]);
  if (at < 0) {
    return ANALOG_MODES[0];
  }
  const length = ANALOG_MODES.length;
  return ANALOG_MODES[(at + direction + length) % length] ?? ANALOG_MODES[0];
}

export async function retypeNode(
  node: string,
  channelType: string,
  act: typeof nodeAction = nodeAction,
): Promise<void> {
  await act(node, { action: "retype", with: { channel_type: channelType } });
}
