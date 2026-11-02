import { Plus, X } from "lucide-react";
import type { IfFilter, IfNotch } from "../lib/types";
import { Button } from "./BaseControls";
import { Checkbox } from "./Checkbox";
import {
  IF_LIMITS,
  type IfEdge,
  ifEdgeHz,
  ifFilterSummary,
  withIfEdge,
  withIfNotchAdded,
  withIfNotchAt,
  withIfNotchRemoved,
} from "./channelSettings";
import { CHIP_SETTING, ICON_BTN_SM } from "./controls";
import { ChipField, SettingChip } from "./face/Chips";
import { Icon } from "./Icon";
import { NumberField } from "./NumberField";
import { rowKeys } from "./rowKeys";

const KHZ = 1_000;
const MHZ = 1_000_000;

export function IfFilterChip({
  filter,
  bandwidthHz,
  channelHz,
  onFilter,
}: {
  filter: IfFilter;
  bandwidthHz: number;
  channelHz: number;
  onFilter: (filter: IfFilter) => void;
}) {
  const enabled = filter.enabled ?? false;
  const summary = ifFilterSummary(filter, bandwidthHz);
  return (
    <SettingChip
      label="IF"
      value={summary}
      quiet={summary === "off"}
      width="w-72"
      title="IF edges and notches ahead of the demodulator"
    >
      {() => (
        <>
          <ChipField label="IF filter">
            <Checkbox
              label="IF filter"
              checked={enabled}
              onChange={(next) => onFilter({ ...filter, enabled: next })}
            />
          </ChipField>
          <EdgeFields filter={filter} bandwidthHz={bandwidthHz} onFilter={onFilter} />
          <NotchRows filter={filter} onFilter={onFilter} />
          <Button
            type="button"
            className={CHIP_SETTING}
            disabled={(filter.notches?.length ?? 0) >= IF_LIMITS.maxNotches}
            title={`Add a notch at the channel frequency, up to ${IF_LIMITS.maxNotches}`}
            onClick={() => {
              const next = withIfNotchAdded(filter, channelHz);
              if (next !== null) {
                onFilter(next);
              }
            }}
          >
            <Icon glyph={Plus} size={12} />
            <span className="font-sans">Notch</span>
          </Button>
        </>
      )}
    </SettingChip>
  );
}

function EdgeFields({
  filter,
  bandwidthHz,
  onFilter,
}: {
  filter: IfFilter;
  bandwidthHz: number;
  onFilter: (filter: IfFilter) => void;
}) {
  const edge = (side: IfEdge) => ifEdgeHz(filter, side, bandwidthHz);
  const invalid = edge("low_hz") >= edge("high_hz");
  const field = (side: IfEdge, label: string) => (
    <NumberField
      label={label}
      unit="kHz"
      value={edge(side) / KHZ}
      min={IF_LIMITS.edgeHz.min / KHZ}
      max={IF_LIMITS.edgeHz.max / KHZ}
      step={0.5}
      invalid={invalid}
      className="w-20"
      onCommit={(khz) => onFilter(withIfEdge(filter, side, khz * KHZ, bandwidthHz))}
    />
  );
  return (
    <ChipField label="IF edges">
      {field("low_hz", "IF low edge")}
      <span className="legend">–</span>
      {field("high_hz", "IF high edge")}
    </ChipField>
  );
}

function NotchRows({
  filter,
  onFilter,
}: {
  filter: IfFilter;
  onFilter: (filter: IfFilter) => void;
}) {
  const notches = filter.notches ?? [];
  const keys = rowKeys(notches);
  return notches.map((notch, index) => (
    <NotchRow
      key={keys[index]}
      name={`IF notch ${index + 1}`}
      notch={notch}
      onEdit={(patch) => onFilter(withIfNotchAt(filter, index, patch))}
      onRemove={() => onFilter(withIfNotchRemoved(filter, index))}
    />
  ));
}

function NotchRow({
  name,
  notch,
  onEdit,
  onRemove,
}: {
  name: string;
  notch: IfNotch;
  onEdit: (patch: Partial<IfNotch>) => void;
  onRemove: () => void;
}) {
  return (
    <ChipField label={name}>
      <NumberField
        label={`${name} frequency`}
        unit="MHz"
        value={notch.frequency_hz / MHZ}
        min={0}
        step={0.0001}
        className="w-28"
        onCommit={(mhz) => onEdit({ frequency_hz: mhz * MHZ })}
      />
      <NumberField
        label={`${name} width`}
        unit="kHz"
        value={(notch.width_hz ?? IF_LIMITS.notchWidthSeedHz) / KHZ}
        min={IF_LIMITS.notchWidthHz.min / KHZ}
        max={IF_LIMITS.notchWidthHz.max / KHZ}
        step={0.1}
        className="w-16"
        onCommit={(khz) => onEdit({ width_hz: khz * KHZ })}
      />
      <Button
        type="button"
        className={`${ICON_BTN_SM} ml-auto hover:text-danger`}
        aria-label={`Remove ${name.toLowerCase()}`}
        onClick={onRemove}
      >
        <Icon glyph={X} size={12} />
      </Button>
    </ChipField>
  );
}
