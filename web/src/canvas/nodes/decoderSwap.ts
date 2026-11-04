import { nodeAction } from "../../lib/api";
import type { ChannelDescriptor } from "../../lib/types";

export function analogModes(types: readonly ChannelDescriptor[]): string[] {
  return types.filter((type) => type.analog_voice === true).map((type) => type.type_id);
}

/** The analog voice mode `direction` steps from `current`, the first one from any other. */
export function nextAnalogMode(
  current: string,
  direction: number,
  types: readonly ChannelDescriptor[],
): string | null {
  const modes = analogModes(types);
  const at = modes.indexOf(current);
  if (at < 0) {
    return modes[0] ?? null;
  }
  return modes[(at + direction + modes.length) % modes.length] ?? null;
}

export async function retypeNode(
  node: string,
  channelType: string,
  act: typeof nodeAction = nodeAction,
): Promise<void> {
  await act(node, { action: "retype", with: { channel_type: channelType } });
}
