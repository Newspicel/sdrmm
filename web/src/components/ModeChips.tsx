import type { ComponentType } from "react";
import type { BroadcastStatus, ChannelParams } from "../lib/types";
import { settledParams } from "./channelSettings";
import { type DecoderChips, MODE_CHIPS } from "./decoders";
import type { ChannelTypeId, Choices, Limits } from "./decoders/kit";

export { bandwidthOptions, serviceOptions } from "./decoders/kit";

export interface ModeChipsProps {
  params: ChannelParams;
  defaults?: ChannelParams;
  broadcast?: BroadcastStatus;
  limits: Limits;
  choices: Choices;
  onParams: (params: ChannelParams) => void;
}

export function ModeChips(props: ModeChipsProps) {
  const listed = MODE_CHIPS[props.params.type] as DecoderChips<ChannelTypeId> | null;
  if (listed === null) {
    return null;
  }
  const Chips = listed as ComponentType<ModeChipsProps>;
  return <Chips {...props} params={settledParams(props.params, props.defaults)} />;
}
