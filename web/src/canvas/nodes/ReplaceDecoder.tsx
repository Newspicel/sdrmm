import { formatHz } from "../../components/format";
import { toastError } from "../../lib/toasts";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { decoderReplacements } from "../palette";
import { ChannelPicker } from "./ChannelPicker";
import { retypeNode } from "./decoderSwap";

export function ReplaceDecoder({ node, onClose }: { node: PatchNode; onClose: () => void }) {
  const workspace = useWorkspaceContext();
  if (node.kind !== "channel") {
    return null;
  }
  const typeId = node.data.channel_type;
  const descriptor = workspace.context.channelTypes.find((type) => type.type_id === typeId);
  const settings =
    workspace.channels.get(node.id)?.settings ?? workspace.savedChannels.get(node.id) ?? null;

  return (
    <ChannelPicker
      title="Replace the decoder"
      note={`${descriptor?.name ?? typeId.toUpperCase()}${
        settings === null ? "" : `: ${formatHz(settings.frequency_hz)}`
      }`}
      groups={decoderReplacements(workspace.context.channelTypes, typeId)}
      onChannel={(channelType) => {
        retypeNode(node.id, channelType).catch(toastError);
        onClose();
      }}
      onClose={onClose}
    />
  );
}
