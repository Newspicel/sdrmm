import { InspectSection } from "../../canvas/inspector";
import {
  BASEMAP_PRESETS,
  type BasemapPreset,
  PRESET_COLORS,
  withCustom,
} from "../../lib/map/basemap";
import { setMapView, useMapView } from "../../lib/map/mapView";
import { Chips, ToggleChip } from "../face/Chips";
import { TextChip } from "../face/TextChip";
import { Swatch } from "../PlotSettings";

const CUSTOM_FILL =
  "repeating-linear-gradient(45deg, var(--color-line-strong) 0 3px, transparent 3px 6px)";

function presetFill(preset: BasemapPreset): string {
  const { land, water } = PRESET_COLORS[preset];
  return `linear-gradient(to right, ${land} 0 60%, ${water} 60%)`;
}

/** The map's look, as inspector sections. Every map shares it. */
export function MapSettings() {
  const view = useMapView();
  const { basemap } = view;
  return (
    <>
      <InspectSection label="Map style">
        <div className="grid grid-cols-3 gap-1.5">
          {BASEMAP_PRESETS.map((preset) => (
            <Swatch
              key={preset}
              name={preset}
              fill={presetFill(preset)}
              on={basemap.preset === preset}
              onClick={() => setMapView({ basemap: { ...basemap, preset } })}
            />
          ))}
          <Swatch
            name="custom"
            fill={CUSTOM_FILL}
            on={basemap.preset === "custom"}
            onClick={() => setMapView({ basemap: { ...basemap, preset: "custom" } })}
          />
        </div>
        {basemap.preset === "custom" && (
          <Chips inset="none">
            <TextChip
              label="URL"
              title="XYZ tile template ({z}/{x}/{y}) or MapLibre style URL. API keys go in the URL."
              value={basemap.custom}
              placeholder="https://…/{z}/{x}/{y}.png"
              onCommit={(custom) => setMapView({ basemap: withCustom(basemap, custom) })}
            />
          </Chips>
        )}
      </InspectSection>
      <InspectSection label="Map">
        <Chips inset="none">
          <ToggleChip
            label="Tracks"
            title="Path flown or sailed by each target"
            on={view.tracks}
            onChange={(tracks) => setMapView({ tracks })}
          />
          <ToggleChip
            label="Globe"
            title="Draw the earth as a globe"
            on={view.globe}
            onChange={(globe) => setMapView({ globe })}
          />
        </Chips>
      </InspectSection>
    </>
  );
}
