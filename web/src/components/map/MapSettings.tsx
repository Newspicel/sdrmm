import {
  BASEMAP_PRESETS,
  type BasemapPreset,
  DEFAULT_BASEMAP,
  PRESET_COLORS,
  withCustom,
} from "../../lib/map/basemap";
import { DEFAULT_MAP_VIEW, setMapView, useMapView } from "../../lib/map/mapView";
import { PlotSettings, Swatch } from "../PlotSettings";
import { SettingsSection } from "../SettingsPanel";
import { Switch } from "../Switch";
import { TextField } from "../TextField";

const CUSTOM_FILL =
  "repeating-linear-gradient(45deg, var(--color-line-strong) 0 3px, transparent 3px 6px)";

function presetFill(preset: BasemapPreset): string {
  const { land, water } = PRESET_COLORS[preset];
  return `linear-gradient(to right, ${land} 0 60%, ${water} 60%)`;
}

export function MapSettings() {
  const view = useMapView();
  const { basemap } = view;
  const changed =
    basemap.preset !== DEFAULT_BASEMAP.preset || view.tracks !== DEFAULT_MAP_VIEW.tracks;
  return (
    <PlotSettings title="Map settings" changed={changed}>
      <SettingsSection name="Style">
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
      </SettingsSection>
      <SettingsSection
        name="Custom"
        hint="XYZ tile template ({z}/{x}/{y}) or MapLibre style URL. API keys go in the URL."
      >
        <TextField
          label="Custom map URL"
          value={basemap.custom}
          placeholder="https://…/{z}/{x}/{y}.png"
          className="w-full"
          onCommit={(custom) => setMapView({ basemap: withCustom(basemap, custom) })}
        />
      </SettingsSection>
      <SettingsSection
        name="Tracks"
        hint="Path flown or sailed by each target"
        aside={
          <Switch
            label="Tracks"
            checked={view.tracks}
            onChange={(tracks) => setMapView({ tracks })}
          />
        }
      />
    </PlotSettings>
  );
}
