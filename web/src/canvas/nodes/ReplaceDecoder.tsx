import { channelFrequencyHz } from "../../components/channelSettings";
import { formatHz } from "../../components/format";
import { toastError } from "../../lib/toasts";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceSelect } from "../context";
import { decoderReplacements } from "../palette";
import { ChannelPicker } from "./ChannelPicker";
import { retypeNode } from "./decoderSwap";

export function ReplaceDecoder({ node, onClose }: { node: PatchNode; onClose: () => void }) {
  const channelTypes = useWorkspaceSelect((workspace) => workspace.context.channelTypes);
  const families = useWorkspaceSelect((workspace) => workspace.context.catalog.families);
  const settings = useWorkspaceSelect(
    (workspace) =>
      workspace.channels.get(node.id)?.settings ?? workspace.savedChannels.get(node.id) ?? null,
  );
  if (node.kind !== "channel") {
    return null;
  }
  const typeId = node.data.channel_type;
  const descriptor = channelTypes.find((type) => type.type_id === typeId);

  return (
    <ChannelPicker
      title="Replace the decoder"
      note={`${descriptor?.name ?? typeId.toUpperCase()}${
        settings === null ? "" : `: ${formatHz(channelFrequencyHz(settings))}`
      }`}
      groups={decoderReplacements(families ?? [], channelTypes, typeId)}
      onChannel={(channelType) => {
        retypeNode(node.id, channelType).catch(toastError);
        onClose();
      }}
      onClose={onClose}
    />
  );
}
