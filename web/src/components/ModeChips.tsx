import { SONDE_BURST_M } from "../lib/limits";
import type {
  BroadcastStatus,
  ChannelParams,
  LoraCodingRate,
  ParamChoice,
  ParamLimit,
  SondeType,
  SstvMode,
} from "../lib/types";
import {
  type ChannelTypeId,
  choiceOptions,
  choicesOf,
  limitOf,
  type NumberLimit,
  scaledLimit,
  unsetAs,
} from "./channelSettings";
import type { Options } from "./controls";
import {
  ChipField,
  ChoiceChip,
  NumberChip,
  OptionalNumberChip,
  SettingChip,
  ToggleChip,
} from "./face/Chips";
import { formatHz } from "./format";
import { LoraKeysChip } from "./LoraKeysChip";
import {
  CTCSS_DEFAULT_HZ,
  DCS_DEFAULT_CODE,
  INVERSION_DEFAULT_HZ,
  LORA_CUSTOM,
  LORA_PRESET_OPTIONS,
  loraPresetOf,
  withLoraPreset,
} from "./modeOptions";
import { NumberField } from "./NumberField";
import { Segmented } from "./Segmented";
import { withCurrent } from "./selectOptions";
import { TextAutocomplete } from "./TextAutocomplete";

type Limits = readonly ParamLimit[];
type Choices = readonly ParamChoice[];

const SSTV_AUTO = "auto";
const SONDE_AUTO = "auto";
type ParamsOf<K extends ChannelTypeId> = Extract<ChannelParams, { type: K }>;

interface Mode<K extends ChannelTypeId> {
  params: ParamsOf<K>;
  limits: Limits;
  choices: Choices;
  onParams: (params: ChannelParams) => void;
}

const INVERT_TITLE = "Flip the signal's polarity; try it when nothing decodes";
const SERVICE_TITLE =
  "Select a discovered audio, video or data service; Auto chooses the first playable service";

export function ModeChips({
  params,
  broadcast,
  limits,
  choices,
  onParams,
}: {
  params: ChannelParams;
  broadcast?: BroadcastStatus;
  limits: Limits;
  choices: Choices;
  onParams: (params: ChannelParams) => void;
}) {
  const mode = { limits, choices, onParams };
  switch (params.type) {
    case "nfm":
      return <NfmChips params={params} {...mode} />;
    case "selcall":
      return <SelcallChips params={params} {...mode} />;
    case "am":
      return <AmChips params={params} {...mode} />;
    case "ssb":
      return <SsbChips params={params} {...mode} />;
    case "wfm":
      return <WfmChips params={params} {...mode} />;
    case "pocsag":
      return <PocsagChips params={params} {...mode} />;
    case "flex":
    case "ermes":
      return <PagerChips params={params} {...mode} />;
    case "eot":
      return <EotChips params={params} {...mode} />;
    case "ism":
      return <IsmChips params={params} {...mode} />;
    case "sarsat":
      return <SarsatChips params={params} {...mode} />;
    case "tetrapol":
      return <TetrapolChips params={params} {...mode} />;
    case "adsb":
      return <AdsbChips params={params} {...mode} />;
    case "ais":
      return <AisChips params={params} {...mode} />;
    case "inmarsat_aero":
      return <AeroChips params={params} {...mode} />;
    case "iridium":
      return <IridiumChips params={params} {...mode} />;
    case "aprs":
      return <AprsChips params={params} {...mode} />;
    case "rtty":
      return <RttyChips params={params} {...mode} />;
    case "morse":
      return <MorseChips params={params} {...mode} />;
    case "cw_skimmer":
      return <SkimmerChips params={params} {...mode} />;
    case "ft8":
    case "ft4":
    case "wspr":
      return <WsjtChips params={params} {...mode} />;
    case "psk":
      return <PskChips params={params} {...mode} />;
    case "navtex":
      return <NavtexChips params={params} {...mode} />;
    case "radio_clock":
      return <RadioClockChips params={params} {...mode} />;
    case "gnss":
      return <GnssChips params={params} {...mode} />;
    case "vor":
      return <VorChips params={params} {...mode} />;
    case "ils":
      return <IlsChips params={params} {...mode} />;
    case "acars":
      return <AcarsChips params={params} {...mode} />;
    case "atv":
      return <AtvChips params={params} {...mode} />;
    case "sstv":
      return <SstvChips params={params} {...mode} />;
    case "dab":
      return <DabChips params={params} broadcast={broadcast} {...mode} />;
    case "datv":
      return <DatvChips params={params} broadcast={broadcast} {...mode} />;
    case "dvbt":
      return <DvbtChips params={params} broadcast={broadcast} {...mode} />;
    case "drm":
      return <DrmChips params={params} broadcast={broadcast} {...mode} />;
    case "dmr":
      return <DmrChips params={params} {...mode} />;
    case "nxdn":
      return <NxdnChips params={params} {...mode} />;
    case "freedv":
      return <FreedvChips params={params} {...mode} />;
    case "ident":
      return <IdentChips params={params} {...mode} />;
    case "dect":
      return <DectChips params={params} {...mode} />;
    case "gsm":
      return <GsmChips params={params} {...mode} />;
    case "apt":
      return <AptChips params={params} {...mode} />;
    case "lrpt":
      return <LrptChips params={params} {...mode} />;
    case "wefax":
      return <WefaxChips params={params} {...mode} />;
    case "radiosonde":
      return <RadiosondeChips params={params} {...mode} />;
    case "lora":
      return <LoraChips params={params} {...mode} />;
    case "remote_id":
      return <RemoteIdChips params={params} {...mode} />;
    case "ble":
      return <BleChips params={params} {...mode} />;
    case "wifi_occupancy":
    case "ism_survey":
      return <AirtimeChips params={params} {...mode} />;
    case "noise_figure":
      return <NoiseFigureChips params={params} {...mode} />;
    case "per":
      return <PerChips params={params} {...mode} />;
    case "sid":
      return <SidChips params={params} {...mode} />;
    case "stanag":
      return <StanagChips params={params} {...mode} />;
    case "dstar":
    case "ysf":
    case "p25":
    case "dpmr":
    case "m17":
    case "tetra":
    case "dsc":
    case "inmarsat_stdc":
    case "vdl2":
    case "hfdl":
      return null;
    default:
      return unhandledMode(params);
  }
}

function unhandledMode(_params: never): null {
  return null;
}

function NfmChips({ params, choices, limits, onParams }: Mode<"nfm">) {
  const settings = params.settings;
  const mode = settings.tone_mode ?? "off";
  const scrambler = settings.scrambler_mode ?? "off";
  const set = (next: typeof settings) => onParams({ type: "nfm", settings: next });
  return (
    <>
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 12_500}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <ChoiceChip
        label="Tone"
        title="Tone squelch"
        value={mode}
        options={choicesOf(choices, params.settings, "tone_mode")}
        quiet={mode === "off"}
        onChange={(tone_mode) =>
          set({
            ...settings,
            tone_mode,
            ctcss_hz: settings.ctcss_hz ?? CTCSS_DEFAULT_HZ,
            dcs_code: settings.dcs_code ?? DCS_DEFAULT_CODE,
          })
        }
      />
      {mode === "ctcss" && (
        <ChoiceChip
          label="CTCSS"
          title="CTCSS tone"
          value={settings.ctcss_hz ?? CTCSS_DEFAULT_HZ}
          options={choicesOf(choices, params.settings, "ctcss_hz")}
          onChange={(ctcss_hz) => set({ ...settings, ctcss_hz })}
        />
      )}
      {mode === "dcs" && (
        <ChoiceChip
          label="DCS"
          title="DCS code"
          value={settings.dcs_code ?? DCS_DEFAULT_CODE}
          options={choicesOf(choices, params.settings, "dcs_code")}
          onChange={(dcs_code) => set({ ...settings, dcs_code })}
        />
      )}
      <ChoiceChip
        label="Scrambler"
        title="Voice scrambler"
        value={scrambler}
        options={choicesOf(choices, params.settings, "scrambler_mode")}
        quiet={scrambler === "off"}
        onChange={(scrambler_mode) =>
          set({
            ...settings,
            scrambler_mode,
            inversion_hz: settings.inversion_hz ?? INVERSION_DEFAULT_HZ,
          })
        }
      />
      {scrambler === "inversion" && (
        <HzChip
          label="Carrier"
          title="Inversion carrier"
          value={settings.inversion_hz ?? INVERSION_DEFAULT_HZ}
          limit={limitOf(limits, "inversion_hz")}
          onCommit={(inversion_hz) => set({ ...settings, inversion_hz })}
        />
      )}
      <ToggleChip
        label="Compander"
        title="Expand audio that was sent with 2:1 compression"
        on={settings.compander ?? false}
        onChange={(compander) => set({ ...settings, compander })}
      />
    </>
  );
}

function SelcallChips({ params, choices, onParams }: Mode<"selcall">) {
  return (
    <ChoiceChip
      label="Plan"
      title="Selective calling tone plan"
      value={params.settings.system ?? "ccir1"}
      options={choicesOf(choices, params.settings, "system")}
      onChange={(system) => onParams({ type: "selcall", settings: { ...params.settings, system } })}
    />
  );
}

function AmChips({ params, onParams }: Mode<"am">) {
  const settings = params.settings;
  return (
    <>
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 10_000}
        optionsHz={[5_000, 8_000, 10_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "am", settings: { ...settings, bandwidth_hz } })
        }
      />
      <ToggleChip
        label="Sync"
        on={settings.sync ?? false}
        title="Lock to the carrier; clearer under fading"
        onChange={(sync) => onParams({ type: "am", settings: { ...settings, sync } })}
      />
    </>
  );
}

function SsbChips({ params, choices, limits, onParams }: Mode<"ssb">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Side"
        title="Sideband"
        value={settings.sideband ?? "usb"}
        options={choicesOf(choices, params.settings, "sideband")}
        onChange={(sideband) => onParams({ type: "ssb", settings: { ...settings, sideband } })}
      />
      <HzChip
        label="BW"
        title="SSB bandwidth"
        value={settings.bandwidth_hz ?? 2_700}
        limit={limitOf(limits, "bandwidth_hz")}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "ssb", settings: { ...settings, bandwidth_hz } })
        }
      />
    </>
  );
}

function WfmChips({ params, choices, onParams }: Mode<"wfm">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="De-emph"
        title="De-emphasis"
        value={settings.deemphasis_us ?? 50}
        options={choicesOf(choices, params.settings, "deemphasis_us")}
        onChange={(deemphasis_us) =>
          onParams({ type: "wfm", settings: { ...settings, deemphasis_us } })
        }
      />
      <ToggleChip
        label="Stereo"
        title="Decode the stereo pilot; mono is quieter on weak signals"
        on={settings.stereo ?? true}
        onChange={(stereo) => onParams({ type: "wfm", settings: { ...settings, stereo } })}
      />
    </>
  );
}

function PocsagChips({ params, choices, onParams }: Mode<"pocsag">) {
  const settings = params.settings;
  const baud = settings.baud ?? "auto";
  return (
    <>
      <ChoiceChip
        label="Baud"
        title="POCSAG baud"
        value={baud}
        options={choicesOf(choices, params.settings, "baud")}
        quiet={baud === "auto"}
        onChange={(next) => onParams({ type: "pocsag", settings: { ...settings, baud: next } })}
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 12_500}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "pocsag", settings: { ...settings, bandwidth_hz } })
        }
      />
      <InvertChip
        on={settings.invert ?? false}
        onChange={(invert) => onParams({ type: "pocsag", settings: { ...settings, invert } })}
      />
    </>
  );
}

function PagerChips({ params, onParams }: Mode<"flex" | "ermes">) {
  const { type, settings } = params;
  return (
    <>
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 12_500}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) => onParams({ type, settings: { ...settings, bandwidth_hz } })}
      />
      <InvertChip
        on={settings.invert ?? false}
        onChange={(invert) => onParams({ type, settings: { ...settings, invert } })}
      />
    </>
  );
}

function EotChips({ params, onParams }: Mode<"eot">) {
  const { settings } = params;
  return (
    <BandwidthChip
      valueHz={settings.bandwidth_hz ?? 12_500}
      optionsHz={[12_500, 25_000]}
      onCommit={(bandwidth_hz) =>
        onParams({ type: "eot", settings: { ...settings, bandwidth_hz } })
      }
    />
  );
}

function IsmChips({ params, onParams }: Mode<"ism">) {
  return (
    <ToggleChip
      label="Unknown"
      title="Also log repeated codes no decoder knows"
      on={params.settings.unknown ?? true}
      onChange={(unknown) => onParams({ type: "ism", settings: { ...params.settings, unknown } })}
    />
  );
}

function SarsatChips({ params, onParams }: Mode<"sarsat">) {
  const { settings } = params;
  return (
    <BandwidthChip
      valueHz={settings.bandwidth_hz ?? 20_000}
      optionsHz={[10_000, 20_000]}
      onCommit={(bandwidth_hz) =>
        onParams({ type: "sarsat", settings: { ...settings, bandwidth_hz } })
      }
    />
  );
}

function TetrapolChips({ params, choices, limits, onParams }: Mode<"tetrapol">) {
  const { settings } = params;
  const set = (next: typeof settings) => onParams({ type: "tetrapol", settings: next });
  return (
    <>
      <ChoiceChip
        label="Band"
        title="Tetrapol band"
        value={settings.band ?? "uhf"}
        options={choicesOf(choices, params.settings, "band")}
        onChange={(band) => set({ ...settings, band })}
      />
      <OptionalNumberChip
        label="Scrambling"
        title="Scrambling code, empty to find it"
        placeholder="auto"
        value={settings.scrambling ?? null}
        {...limitOf(limits, "scrambling")}
        onCommit={(scrambling) => set({ ...settings, scrambling })}
      />
    </>
  );
}

function AdsbChips({ params, onParams }: Mode<"adsb">) {
  return (
    <ToggleChip
      label="CRC fix"
      title="Repair single-bit errors the checksum can pin down"
      on={params.settings.crc_fix ?? true}
      onChange={(crc_fix) => onParams({ type: "adsb", settings: { ...params.settings, crc_fix } })}
    />
  );
}

function AisChips({ params, choices, onParams }: Mode<"ais">) {
  return (
    <ChoiceChip
      label="Channel"
      title="AIS channel"
      value={params.settings.ais_channel ?? "a"}
      options={choicesOf(choices, params.settings, "ais_channel")}
      onChange={(ais_channel) =>
        onParams({ type: "ais", settings: { ...params.settings, ais_channel } })
      }
    />
  );
}

function AeroChips({ params, choices, onParams }: Mode<"inmarsat_aero">) {
  return (
    <ChoiceChip
      label="Channel"
      title="Aero channel"
      value={params.settings.channel ?? "p"}
      options={choicesOf(choices, params.settings, "channel")}
      onChange={(channel) =>
        onParams({ type: "inmarsat_aero", settings: { ...params.settings, channel } })
      }
    />
  );
}

function IridiumChips({ params, choices, onParams }: Mode<"iridium">) {
  return (
    <ChoiceChip
      label="Span"
      title="Iridium span"
      value={params.settings.span ?? "channel"}
      options={choicesOf(choices, params.settings, "span")}
      onChange={(span) => onParams({ type: "iridium", settings: { ...params.settings, span } })}
    />
  );
}

function AprsChips({ params, choices, onParams }: Mode<"aprs">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Mode"
        title="APRS mode"
        value={settings.mode ?? "afsk1200"}
        options={choicesOf(choices, params.settings, "mode")}
        onChange={(mode) => onParams({ type: "aprs", settings: { ...settings, mode } })}
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 12_500}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "aprs", settings: { ...settings, bandwidth_hz } })
        }
      />
    </>
  );
}

function RttyChips({ params, choices, limits, onParams }: Mode<"rtty">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "rtty", settings: next });
  return (
    <>
      <PresetChip
        label="Baud"
        title="RTTY baud"
        value={settings.baud ?? 45.45}
        presets={choicesOf(choices, params.settings, "baud")}
        limit={limitOf(limits, "baud")}
        onCommit={(baud) => set({ ...settings, baud })}
      />
      <PresetChip
        label="Shift"
        title="RTTY shift"
        unit="Hz"
        value={settings.shift_hz ?? 170}
        presets={choicesOf(choices, params.settings, "shift_hz")}
        limit={limitOf(limits, "shift_hz")}
        onCommit={(shift_hz) => set({ ...settings, shift_hz })}
      />
      <ChoiceChip
        label="Stop"
        title="RTTY stop bits"
        value={settings.stop_bits ?? "one_and_half"}
        options={choicesOf(choices, params.settings, "stop_bits")}
        onChange={(stop_bits) => set({ ...settings, stop_bits })}
      />
      <InvertChip
        on={settings.invert ?? false}
        onChange={(invert) => set({ ...settings, invert })}
      />
      <ToggleChip
        label="Unshift"
        title="Drop back to letters after a space, as most stations expect"
        on={settings.unshift_on_space ?? true}
        onChange={(unshift_on_space) => set({ ...settings, unshift_on_space })}
      />
    </>
  );
}

function MorseChips({ params, limits, onParams }: Mode<"morse">) {
  const settings = params.settings;
  return (
    <>
      <HzChip
        label="BW"
        title="CW filter bandwidth"
        value={settings.bandwidth_hz ?? 400}
        limit={limitOf(limits, "bandwidth_hz")}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "morse", settings: { ...settings, bandwidth_hz } })
        }
      />
      <OptionalNumberChip
        label="Speed"
        title="Morse speed, empty to auto-track"
        placeholder="auto"
        value={settings.wpm ?? null}
        {...limitOf(limits, "wpm")}
        unit="WPM"
        onCommit={(wpm) => onParams({ type: "morse", settings: { ...settings, wpm } })}
      />
    </>
  );
}

function SkimmerChips({ params, limits, onParams }: Mode<"cw_skimmer">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "cw_skimmer", settings: next });
  return (
    <>
      <HzChip
        label="Passband"
        title="CW skimmer passband"
        value={settings.bandwidth_hz ?? 24_000}
        limit={limitOf(limits, "bandwidth_hz")}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <NumberChip
        label="Acquire"
        title="Carrier threshold above the noise floor"
        value={settings.threshold_db ?? 10}
        {...limitOf(limits, "threshold_db")}
        unit="dB SNR"
        onCommit={(threshold_db) => set({ ...settings, threshold_db })}
      />
      <NumberChip
        label="Signals"
        title="Maximum simultaneous CW signals"
        value={settings.max_signals ?? 32}
        {...limitOf(limits, "max_signals")}
        onCommit={(max_signals) => set({ ...settings, max_signals })}
      />
      <OptionalNumberChip
        label="Speed"
        title="Morse speed, empty to track each signal"
        placeholder="auto"
        value={settings.wpm ?? null}
        {...limitOf(limits, "wpm")}
        unit="WPM"
        onCommit={(wpm) => set({ ...settings, wpm })}
      />
    </>
  );
}

function WsjtChips({ params, limits, onParams }: Mode<"ft8" | "ft4" | "wspr">) {
  const { type, settings } = params;
  const wspr = type === "wspr";
  return (
    <>
      <HzChip
        label="From"
        title="Lowest USB audio frequency searched"
        value={settings.audio_low_hz ?? (wspr ? 1_400 : 200)}
        limit={limitOf(limits, "audio_low_hz")}
        onCommit={(audio_low_hz) => onParams({ type, settings: { ...settings, audio_low_hz } })}
      />
      <HzChip
        label="To"
        title="Highest USB audio frequency searched"
        value={settings.audio_high_hz ?? (wspr ? 1_600 : 3_000)}
        limit={limitOf(limits, "audio_high_hz")}
        onCommit={(audio_high_hz) => onParams({ type, settings: { ...settings, audio_high_hz } })}
      />
      <NumberChip
        label="Candidates"
        title="Maximum synchronized signals tried per decode pass"
        value={settings.max_candidates ?? 200}
        {...limitOf(limits, "max_candidates")}
        onCommit={(max_candidates) => onParams({ type, settings: { ...settings, max_candidates } })}
      />
    </>
  );
}

function PskChips({ params, choices, onParams }: Mode<"psk">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Mode"
        title="PSK symbol rate"
        value={settings.baud ?? "psk31"}
        options={choicesOf(choices, params.settings, "baud")}
        onChange={(baud) => onParams({ type: "psk", settings: { ...settings, baud } })}
      />
      <InvertChip
        on={settings.invert ?? false}
        onChange={(invert) => onParams({ type: "psk", settings: { ...settings, invert } })}
      />
    </>
  );
}

function NavtexChips({ params, onParams }: Mode<"navtex">) {
  return (
    <InvertChip
      on={params.settings.invert ?? false}
      onChange={(invert) => onParams({ type: "navtex", settings: { ...params.settings, invert } })}
    />
  );
}

function RadioClockChips({ params, choices, onParams }: Mode<"radio_clock">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Service"
        title="Radio clock service"
        value={settings.standard ?? "dcf77"}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) =>
          onParams({ type: "radio_clock", settings: { ...settings, standard } })
        }
      />
      <InvertChip
        on={settings.invert ?? false}
        onChange={(invert) => onParams({ type: "radio_clock", settings: { ...settings, invert } })}
      />
    </>
  );
}

function GnssChips({ params, limits, onParams }: Mode<"gnss">) {
  const settings = params.settings;
  return (
    <>
      <NumberChip
        label="PRN"
        title="GPS L1 C/A satellite PRN"
        value={settings.prn ?? 1}
        {...limitOf(limits, "prn")}
        onCommit={(prn) => onParams({ type: "gnss", settings: { ...settings, prn } })}
      />
      <HzChip
        label="Doppler"
        title="Symmetric Doppler search span"
        value={settings.doppler_hz ?? 10_000}
        limit={limitOf(limits, "doppler_hz")}
        onCommit={(doppler_hz) => onParams({ type: "gnss", settings: { ...settings, doppler_hz } })}
      />
      <NumberChip
        label="Acquire"
        title="Correlation peak-to-floor acquisition threshold"
        value={settings.threshold ?? 4}
        {...limitOf(limits, "threshold")}
        unit="× floor"
        onCommit={(threshold) => onParams({ type: "gnss", settings: { ...settings, threshold } })}
      />
    </>
  );
}

function VorChips({ params, limits, onParams }: Mode<"vor">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "vor", settings: next });
  const station = settings.station ?? "";
  return (
    <>
      <SettingChip
        label="Station"
        value={station === "" ? "none" : station}
        quiet={station === ""}
        title="VOR station identifier"
      >
        {() => (
          <ChipField label="VOR station identifier">
            <TextAutocomplete
              className="min-w-0 flex-1"
              label="VOR station identifier"
              value={station}
              suggestions={[]}
              placeholder="Optional identifier"
              onCommit={(next) => {
                set({ ...settings, station: next === "" ? undefined : next });
                return true;
              }}
            />
          </ChipField>
        )}
      </SettingChip>
      <OptionalNumberChip
        label="Lat"
        title="VOR station latitude"
        placeholder="Unknown"
        value={settings.station_lat ?? null}
        {...limitOf(limits, "station_lat")}
        unit="°"
        onCommit={(station_lat) => set({ ...settings, station_lat })}
      />
      <OptionalNumberChip
        label="Lon"
        title="VOR station longitude"
        placeholder="Unknown"
        value={settings.station_lon ?? null}
        {...limitOf(limits, "station_lon")}
        unit="°"
        onCommit={(station_lon) => set({ ...settings, station_lon })}
      />
      <NumberChip
        label="Declination"
        title="East-positive magnetic declination at the VOR"
        value={settings.magnetic_declination_deg ?? 0}
        {...limitOf(limits, "magnetic_declination_deg")}
        unit="°"
        onCommit={(magnetic_declination_deg) => set({ ...settings, magnetic_declination_deg })}
      />
      <NumberChip
        label="Every"
        title="VOR report interval"
        value={settings.report_ms ?? 500}
        {...limitOf(limits, "report_ms")}
        unit="ms"
        onCommit={(report_ms) => set({ ...settings, report_ms })}
      />
    </>
  );
}

function IlsChips({ params, choices, limits, onParams }: Mode<"ils">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Component"
        title="ILS component"
        value={settings.component ?? "localizer"}
        options={choicesOf(choices, params.settings, "component")}
        onChange={(component) => onParams({ type: "ils", settings: { ...settings, component } })}
      />
      <NumberChip
        label="Every"
        title="ILS report interval"
        value={settings.report_ms ?? 500}
        {...limitOf(limits, "report_ms")}
        unit="ms"
        onCommit={(report_ms) => onParams({ type: "ils", settings: { ...settings, report_ms } })}
      />
    </>
  );
}

function AcarsChips({ params, onParams }: Mode<"acars">) {
  return (
    <BandwidthChip
      valueHz={params.settings.bandwidth_hz ?? 12_500}
      optionsHz={[8_000, 12_500, 25_000]}
      onCommit={(bandwidth_hz) =>
        onParams({ type: "acars", settings: { ...params.settings, bandwidth_hz } })
      }
    />
  );
}

function AtvChips({ params, choices, limits, onParams }: Mode<"atv">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "atv", settings: next });
  return (
    <>
      <ChoiceChip
        label="Mod"
        title="Modulation"
        value={settings.modulation ?? "am"}
        options={choicesOf(choices, params.settings, "modulation")}
        onChange={(modulation) => set({ ...settings, modulation })}
      />
      <ChoiceChip
        label="Lines"
        title="Scanning standard"
        value={settings.standard ?? "ccir625"}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) => set({ ...settings, standard })}
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 1_500_000}
        optionsHz={[500_000, 1_000_000, 1_500_000, 1_600_000]}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <ChoiceChip
        label="Colour"
        title="Composite colour system"
        value={settings.color ?? "monochrome"}
        options={choicesOf(choices, params.settings, "color")}
        onChange={(color) => set({ ...settings, color })}
      />
      <OptionalNumberChip
        label="Sound"
        title="FM sound subcarrier, empty for none"
        placeholder="off"
        value={
          settings.sound_subcarrier_hz == null ? null : settings.sound_subcarrier_hz / 1_000_000
        }
        {...scaledLimit(limitOf(limits, "sound_subcarrier_hz"), 1e-6)}
        unit="MHz"
        onCommit={(mhz) =>
          set({ ...settings, sound_subcarrier_hz: mhz === null ? null : mhz * 1_000_000 })
        }
      />
      <ToggleChip
        label="Interlace"
        title="Weave both fields into one frame"
        on={settings.interlace ?? true}
        onChange={(interlace) => set({ ...settings, interlace })}
      />
      <InvertChip
        on={settings.invert ?? false}
        onChange={(invert) => set({ ...settings, invert })}
      />
    </>
  );
}

function SstvChips({ params, choices, onParams }: Mode<"sstv">) {
  const settings = params.settings;
  const mode = settings.mode ?? SSTV_AUTO;
  return (
    <>
      <ChoiceChip
        label="Mod"
        title="Modulation"
        value={settings.modulation ?? "usb"}
        options={choicesOf(choices, params.settings, "modulation")}
        onChange={(modulation) => onParams({ type: "sstv", settings: { ...settings, modulation } })}
      />
      <ChoiceChip
        label="Mode"
        title="Scanning mode"
        value={mode}
        options={unsetAs(choiceOptions<SstvMode | null>(choices, "mode"), SSTV_AUTO)}
        quiet={mode === SSTV_AUTO}
        onChange={(next) =>
          onParams({
            type: "sstv",
            settings: { ...settings, mode: next === SSTV_AUTO ? null : next },
          })
        }
      />
      <ToggleChip
        label="Slant"
        title="Straighten pictures from a sender whose clock runs off"
        on={settings.slant_correction ?? true}
        onChange={(slant_correction) =>
          onParams({ type: "sstv", settings: { ...settings, slant_correction } })
        }
      />
      <ToggleChip
        label="Partial"
        title="Keep unfinished pictures"
        on={settings.keep_partial ?? true}
        onChange={(keep_partial) =>
          onParams({ type: "sstv", settings: { ...settings, keep_partial } })
        }
      />
    </>
  );
}

function DabChips({
  params,
  choices,
  broadcast,
  onParams,
}: Mode<"dab"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const mode = settings.mode ?? "auto";
  const transmissionMode = settings.transmission_mode ?? "i";
  const detected = broadcast?.transmission_mode;
  const transmissionModes =
    detected == null
      ? choicesOf(choices, settings, "transmission_mode")
      : choicesOf(choices, settings, "transmission_mode").map((option) =>
          option.value === "auto" ? { ...option, label: `Auto ${detected.toUpperCase()}` } : option,
        );
  return (
    <>
      <ChoiceChip
        label="Type"
        title="DAB generation"
        value={mode}
        options={choicesOf(choices, params.settings, "mode")}
        quiet={mode === "auto"}
        onChange={(next) => onParams({ type: "dab", settings: { ...settings, mode: next } })}
      />
      <ChoiceChip
        label="Mode"
        title="DAB transmission mode"
        value={transmissionMode}
        options={transmissionModes}
        quiet={transmissionMode === "auto"}
        onChange={(transmission_mode) =>
          onParams({ type: "dab", settings: { ...settings, transmission_mode } })
        }
      />
      <ServiceChip
        status={broadcast}
        value={settings.service_id ?? null}
        max={0xffffffff}
        onChange={(service_id) => onParams({ type: "dab", settings: { ...settings, service_id } })}
      />
    </>
  );
}

function DatvChips({
  params,
  choices,
  broadcast,
  limits,
  onParams,
}: Mode<"datv"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "datv", settings: next });
  const codeRate = settings.code_rate ?? "auto";
  return (
    <>
      <ChoiceChip
        label="Standard"
        title="DATV standard"
        value={settings.standard ?? "dvb_s"}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) => set({ ...settings, standard })}
      />
      <NumberChip
        label="Rate"
        title="DATV symbol rate"
        value={settings.symbol_rate ?? 333_000}
        {...limitOf(limits, "symbol_rate")}
        unit="Bd"
        onCommit={(symbol_rate) => set({ ...settings, symbol_rate })}
      />
      <ServiceChip
        status={broadcast}
        value={settings.program ?? null}
        max={65535}
        onChange={(program) => set({ ...settings, program })}
      />
      {settings.standard === "dvb_s2" ? (
        <>
          <ChoiceChip
            label="Roll-off"
            title="Match the transmitter's filter shape; DVB-S2 signals it in the base band header"
            value={settings.roll_off ?? "pct35"}
            options={choicesOf(choices, params.settings, "roll_off")}
            onChange={(roll_off) => set({ ...settings, roll_off })}
          />
          <ToggleChip
            label="Superframes"
            title="Receive DVB-S2X Annex E superframes, formats 0 to 7"
            on={settings.superframes ?? false}
            onChange={(superframes) => set({ ...settings, superframes })}
          />
          {settings.superframes ? (
            <>
              <NumberChip
                label="Ref code"
                title="Superframe reference scrambling code n_Ref, 0 by default"
                value={settings.superframe_reference ?? 0}
                {...limitOf(limits, "superframe_reference")}
                onCommit={(superframe_reference) => set({ ...settings, superframe_reference })}
              />
              <NumberChip
                label="Data code"
                title="Superframe payload scrambling code n_Pay, 0 by default"
                value={settings.superframe_payload ?? 0}
                {...limitOf(limits, "superframe_payload")}
                onCommit={(superframe_payload) => set({ ...settings, superframe_payload })}
              />
              <ToggleChip
                label="Code search"
                title="Find unknown scrambling codes from the signal; needs a clean carrier"
                on={settings.superframe_search ?? false}
                onChange={(superframe_search) => set({ ...settings, superframe_search })}
              />
            </>
          ) : null}
          <OptionalNumberChip
            label="Stream"
            title="Choose an input stream identifier on a multistream carrier"
            placeholder="Auto"
            value={settings.input_stream ?? null}
            min={0}
            max={255}
            step={1}
            onCommit={(input_stream) => set({ ...settings, input_stream })}
          />
        </>
      ) : (
        <ChoiceChip
          label="FEC"
          title="DVB-S code rate"
          value={codeRate}
          options={choicesOf(choices, params.settings, "code_rate")}
          quiet={codeRate === "auto"}
          onChange={(code_rate) => set({ ...settings, code_rate })}
        />
      )}
    </>
  );
}

function DvbtChips({
  params,
  choices,
  broadcast,
  onParams,
}: Mode<"dvbt"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "dvbt", settings: next });
  return (
    <>
      <ChoiceChip
        label="Standard"
        title="Terrestrial standard"
        value={settings.standard ?? "dvb_t"}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) => set({ ...settings, standard })}
      />
      <ChoiceChip
        label="BW"
        title="DVB-T bandwidth"
        value={settings.bandwidth ?? "mhz8"}
        options={choicesOf(choices, params.settings, "bandwidth")}
        onChange={(bandwidth) => set({ ...settings, bandwidth })}
      />
      {settings.standard === "dvb_t2" ? (
        <OptionalNumberChip
          label="PLP"
          title="PLP ID, empty for automatic selection"
          placeholder="auto"
          value={settings.plp ?? null}
          min={0}
          max={255}
          step={1}
          onCommit={(plp) => set({ ...settings, plp })}
        />
      ) : (
        <ToggleChip
          label="Low priority"
          title="Decode the low priority transport stream of a hierarchical DVB-T multiplex"
          on={settings.low_priority ?? false}
          onChange={(low_priority) => set({ ...settings, low_priority })}
        />
      )}
      <ServiceChip
        status={broadcast}
        value={settings.program ?? null}
        max={65535}
        onChange={(program) => set({ ...settings, program })}
      />
    </>
  );
}

function DrmChips({
  params,
  choices,
  broadcast,
  onParams,
}: Mode<"drm"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const mode = settings.mode ?? "auto";
  return (
    <>
      <ChoiceChip
        label="Mode"
        title="DRM mode"
        value={mode}
        options={choicesOf(choices, params.settings, "mode")}
        quiet={mode === "auto"}
        onChange={(next) =>
          onParams({
            type: "drm",
            settings: {
              ...settings,
              mode: next,
              bandwidth_hz:
                next === "drm30" ? (mode === "drm30" ? settings.bandwidth_hz : 10_000) : 100_000,
            },
          })
        }
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz ?? 100_000}
        optionsHz={mode === "drm30" ? [4_500, 5_000, 9_000, 10_000, 18_000, 20_000] : [100_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "drm", settings: { ...settings, bandwidth_hz } })
        }
      />
      <ServiceChip
        status={broadcast}
        value={settings.service ?? null}
        max={3}
        onChange={(service) => onParams({ type: "drm", settings: { ...settings, service } })}
      />
    </>
  );
}

function DmrChips({ params, choices, onParams }: Mode<"dmr">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Slot"
        title="Slot"
        value={settings.slots ?? "both"}
        options={choicesOf(choices, params.settings, "slots")}
        onChange={(slots) => onParams({ type: "dmr", settings: { ...settings, slots } })}
      />
      <ToggleChip
        label="Skip CRC"
        title="Ignore data CRC"
        on={settings.ignore_crc ?? false}
        onChange={(ignore_crc) => onParams({ type: "dmr", settings: { ...settings, ignore_crc } })}
      />
    </>
  );
}

function NxdnChips({ params, choices, onParams }: Mode<"nxdn">) {
  return (
    <ChoiceChip
      label="Width"
      title="Width"
      value={params.settings.bandwidth ?? "narrow"}
      options={choicesOf(choices, params.settings, "bandwidth")}
      onChange={(bandwidth) =>
        onParams({ type: "nxdn", settings: { ...params.settings, bandwidth } })
      }
    />
  );
}

function FreedvChips({ params, choices, onParams }: Mode<"freedv">) {
  return (
    <ChoiceChip
      label="Side"
      title="FreeDV sideband"
      value={params.settings.sideband ?? "usb"}
      options={choicesOf(choices, params.settings, "sideband")}
      onChange={(sideband) =>
        onParams({ type: "freedv", settings: { ...params.settings, sideband } })
      }
    />
  );
}

function IdentChips({ params, limits, onParams }: Mode<"ident">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "ident", settings: next });
  return (
    <>
      <BandwidthChip
        label="Width"
        title="Search bandwidth"
        valueHz={settings.bandwidth_hz ?? 192_000}
        optionsHz={[12_500, 50_000, 100_000, 192_000]}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <NumberChip
        label="Every"
        title="Milliseconds of signal each report is measured from"
        value={settings.interval_ms ?? 1_000}
        {...limitOf(limits, "interval_ms")}
        unit="ms"
        onCommit={(interval_ms) => set({ ...settings, interval_ms })}
      />
      <NumberChip
        label="Detect"
        title="Decibels above the noise floor a signal must reach"
        value={settings.threshold_db ?? 8}
        {...limitOf(limits, "threshold_db")}
        unit="dB"
        onCommit={(threshold_db) => set({ ...settings, threshold_db })}
      />
    </>
  );
}

function DectChips({ params, choices, onParams }: Mode<"dect">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Band"
        title="DECT band"
        value={settings.band ?? "eu"}
        options={choicesOf(choices, params.settings, "band")}
        onChange={(band) => onParams({ type: "dect", settings: { ...settings, band } })}
      />
      <ChoiceChip
        label="Span"
        title="DECT span"
        value={settings.span ?? "carrier"}
        options={choicesOf(choices, params.settings, "span")}
        onChange={(span) => onParams({ type: "dect", settings: { ...settings, span } })}
      />
      <ChoiceChip
        label="Side"
        title="DECT side"
        value={settings.sides ?? "both"}
        options={choicesOf(choices, params.settings, "sides")}
        onChange={(sides) => onParams({ type: "dect", settings: { ...settings, sides } })}
      />
    </>
  );
}

function GsmChips({ params, onParams }: Mode<"gsm">) {
  const settings = params.settings;
  return (
    <>
      <ToggleChip
        label="Paging"
        title="Show paging and assignments"
        on={settings.paging ?? true}
        onChange={(paging) => onParams({ type: "gsm", settings: { ...settings, paging } })}
      />
      <ToggleChip
        label="Follow"
        title="Follow assigned SDCCH on this carrier"
        on={settings.follow ?? true}
        onChange={(follow) => onParams({ type: "gsm", settings: { ...settings, follow } })}
      />
    </>
  );
}

function AptChips({ params, onParams }: Mode<"apt">) {
  const settings = params.settings;
  return (
    <ToggleChip
      label="Partial"
      title="Keep unfinished passes"
      on={settings.keep_partial ?? true}
      onChange={(keep_partial) =>
        onParams({ type: "apt", settings: { ...settings, keep_partial } })
      }
    />
  );
}

function LrptChips({ params, choices, onParams }: Mode<"lrpt">) {
  const settings = params.settings;
  return (
    <ChoiceChip
      label="Mode"
      title="Meteor-M downlink mode"
      value={settings.mode ?? "oqpsk72"}
      options={choicesOf(choices, params.settings, "mode")}
      onChange={(mode) => onParams({ type: "lrpt", settings: { ...settings, mode } })}
    />
  );
}

function WefaxChips({ params, choices, onParams }: Mode<"wefax">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "wefax", settings: next });
  return (
    <>
      <ChoiceChip
        label="IOC"
        title="Index of cooperation"
        value={settings.ioc ?? "ioc576"}
        options={choicesOf(choices, params.settings, "ioc")}
        onChange={(ioc) => set({ ...settings, ioc })}
      />
      <ChoiceChip
        label="LPM"
        title="Lines per minute"
        value={settings.lpm ?? "lpm120"}
        options={choicesOf(choices, params.settings, "lpm")}
        onChange={(lpm) => set({ ...settings, lpm })}
      />
      <ToggleChip
        label="Partial"
        title="Keep unfinished charts"
        on={settings.keep_partial ?? true}
        onChange={(keep_partial) => set({ ...settings, keep_partial })}
      />
    </>
  );
}

function RadiosondeChips({ params, choices, limits, onParams }: Mode<"radiosonde">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "radiosonde", settings: next });
  const sonde = settings.sonde ?? SONDE_AUTO;
  return (
    <>
      <ChoiceChip
        label="Sonde"
        title="Sonde type"
        value={sonde}
        options={unsetAs(choiceOptions<SondeType | null>(choices, "sonde"), SONDE_AUTO)}
        quiet={sonde === SONDE_AUTO}
        onChange={(next) => set({ ...settings, sonde: next === SONDE_AUTO ? null : next })}
      />
      <NumberChip
        label="Burst"
        title="Burst altitude assumed while a sonde climbs"
        value={settings.burst_m ?? SONDE_BURST_M}
        {...limitOf(limits, "burst_m")}
        unit="m"
        onCommit={(burst_m) => set({ ...settings, burst_m })}
      />
      <NumberChip
        label="Ground"
        title="Ground altitude where sondes land"
        value={settings.ground_m ?? 0}
        {...limitOf(limits, "ground_m")}
        unit="m"
        onCommit={(ground_m) => set({ ...settings, ground_m })}
      />
    </>
  );
}

function BleChips({ params, choices, onParams }: Mode<"ble">) {
  return (
    <ChoiceChip
      label="Link"
      title="Bluetooth channels to listen on"
      value={params.settings.link ?? "channel"}
      options={choicesOf(choices, params.settings, "link")}
      onChange={(link) => onParams({ type: "ble", settings: { ...params.settings, link } })}
    />
  );
}

function AirtimeChips({
  params,
  choices,
  limits,
  onParams,
}: Mode<"wifi_occupancy" | "ism_survey">) {
  const { type, settings } = params;
  return (
    <>
      <ChoiceChip
        label="Span"
        title="Width of the window measured, in MHz"
        value={settings.span ?? "mhz20"}
        options={choicesOf(choices, params.settings, "span")}
        onChange={(span) => onParams({ type, settings: { ...settings, span } })}
      />
      <NumberChip
        label="Margin"
        title="How far over the noise floor counts as busy"
        value={settings.margin_db ?? (type === "wifi_occupancy" ? 6 : 10)}
        {...limitOf(limits, "margin_db")}
        unit="dB"
        onCommit={(margin_db) => onParams({ type, settings: { ...settings, margin_db } })}
      />
    </>
  );
}

function NoiseFigureChips({ params, limits, onParams }: Mode<"noise_figure">) {
  return (
    <NumberChip
      label="ENR"
      title="Excess noise ratio of the noise source at this frequency"
      value={params.settings.enr_db ?? 15}
      {...limitOf(limits, "enr_db")}
      unit="dB"
      onCommit={(enr_db) => onParams({ type: "noise_figure", settings: { enr_db } })}
    />
  );
}

function PerChips({ params, choices, limits, onParams }: Mode<"per">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "per", settings: next });
  return (
    <>
      <ChoiceChip
        label="Baud"
        title="2-FSK symbol rate of the test frames"
        value={settings.baud ?? "baud9600"}
        options={choicesOf(choices, params.settings, "baud")}
        onChange={(baud) => set({ ...settings, baud })}
      />
      <NumberChip
        label="Payload"
        title="PRBS9 payload length of each frame"
        value={settings.payload_bytes ?? 32}
        {...limitOf(limits, "payload_bytes")}
        unit="B"
        onCommit={(payload_bytes) => set({ ...settings, payload_bytes })}
      />
    </>
  );
}

function StanagChips({ params, limits, onParams }: Mode<"stanag">) {
  return (
    <NumberChip
      label="Window"
      title="Seconds of signal behind each verdict"
      value={params.settings.window_s ?? 2}
      {...limitOf(limits, "window_s")}
      unit="s"
      onCommit={(window_s) => onParams({ type: "stanag", settings: { window_s } })}
    />
  );
}

function SidChips({ params, limits, onParams }: Mode<"sid">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "sid", settings: next });
  return (
    <>
      <NumberChip
        label="Every"
        title="Averaging interval of each report"
        value={settings.report_s ?? 5}
        {...limitOf(limits, "report_s")}
        unit="s"
        onCommit={(report_s) => set({ ...settings, report_s })}
      />
      <NumberChip
        label="Threshold"
        title="Level change against the baseline that counts as a disturbance"
        value={settings.threshold_db ?? 3}
        {...limitOf(limits, "threshold_db")}
        unit="dB"
        onCommit={(threshold_db) => set({ ...settings, threshold_db })}
      />
    </>
  );
}

function RemoteIdChips({ params, choices, onParams }: Mode<"remote_id">) {
  return (
    <ChoiceChip
      label="Link"
      title="Radio link to listen on"
      value={params.settings.link ?? "bluetooth"}
      options={choicesOf(choices, params.settings, "link")}
      onChange={(link) => onParams({ type: "remote_id", settings: { ...params.settings, link } })}
    />
  );
}

function LoraChips({ params, choices, onParams }: Mode<"lora">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "lora", settings: next });
  const preset = loraPresetOf(settings);
  const spreadingFactor = settings.spreading_factor ?? "all";
  const protocol = settings.protocol ?? "auto";
  const header = settings.implicit_header ?? null;
  return (
    <>
      <ChoiceChip
        label="Preset"
        title="Modem preset; frequency stays as tuned"
        value={preset}
        options={LORA_PRESET_OPTIONS}
        quiet={preset === LORA_CUSTOM}
        onChange={(id) => set(withLoraPreset(settings, id))}
      />
      <ChoiceChip
        label="BW"
        title="LoRa bandwidth"
        value={settings.bandwidth ?? "khz125"}
        options={choicesOf(choices, params.settings, "bandwidth")}
        onChange={(bandwidth) => set({ ...settings, bandwidth })}
      />
      <ChoiceChip
        label="SF"
        title="Spreading factor"
        value={spreadingFactor}
        options={choicesOf(choices, params.settings, "spreading_factor")}
        quiet={spreadingFactor === "all"}
        onChange={(spreading_factor) => set({ ...settings, spreading_factor })}
      />
      <ChoiceChip
        label="IQ"
        title="Chirp direction"
        value={settings.iq ?? "normal"}
        options={choicesOf(choices, params.settings, "iq")}
        onChange={(iq) => set({ ...settings, iq })}
      />
      <ChoiceChip
        label="Protocol"
        title="Payload protocol"
        value={protocol}
        options={choicesOf(choices, params.settings, "protocol")}
        quiet={protocol === "auto"}
        onChange={(next) => set({ ...settings, protocol: next })}
      />
      <LoraKeysChip keys={settings.keys ?? []} onChange={(keys) => set({ ...settings, keys })} />
      <OptionalNumberChip
        label="Implicit"
        title="Implicit header payload length, empty for explicit headers"
        placeholder="off"
        value={header?.length ?? null}
        min={1}
        max={255}
        step={1}
        unit="B"
        onCommit={(length) =>
          set({
            ...settings,
            implicit_header:
              length === null
                ? null
                : { coding_rate: "4/5", crc: true, ...header, length: Math.round(length) },
          })
        }
      />
      {header !== null && (
        <>
          <ChoiceChip
            label="CR"
            title="Implicit header coding rate"
            value={header.coding_rate ?? "4/5"}
            options={choiceOptions<LoraCodingRate>(choices, "implicit_header.coding_rate")}
            onChange={(coding_rate) =>
              set({ ...settings, implicit_header: { ...header, coding_rate } })
            }
          />
          <ToggleChip
            label="CRC"
            title="Implicit header payload carries a CRC"
            on={header.crc ?? true}
            onChange={(crc) => set({ ...settings, implicit_header: { ...header, crc } })}
          />
        </>
      )}
    </>
  );
}

export function bandwidthOptions(valueHz: number, optionsHz: readonly number[]): Options<number> {
  return withCurrent(
    valueHz,
    optionsHz.map((hz) => ({ value: hz, label: formatHz(hz) })),
    formatHz,
  );
}

function BandwidthChip({
  label = "BW",
  title = "Channel bandwidth",
  valueHz,
  optionsHz,
  onCommit,
}: {
  label?: string;
  title?: string;
  valueHz: number;
  optionsHz: readonly number[];
  onCommit: (hz: number) => void;
}) {
  return (
    <ChoiceChip
      label={label}
      title={title}
      value={valueHz}
      options={bandwidthOptions(valueHz, optionsHz)}
      onChange={onCommit}
    />
  );
}

function HzChip({
  label,
  title,
  value,
  limit,
  onCommit,
}: {
  label: string;
  title: string;
  value: number;
  limit: NumberLimit;
  onCommit: (hz: number) => void;
}) {
  return (
    <NumberChip
      label={label}
      title={title}
      value={value}
      shown={formatHz(value)}
      unit="Hz"
      {...limit}
      onCommit={onCommit}
    />
  );
}

function InvertChip({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return <ToggleChip label="Invert" title={INVERT_TITLE} on={on} onChange={onChange} />;
}

function PresetChip({
  label,
  title,
  value,
  presets,
  limit,
  unit,
  onCommit,
}: {
  label: string;
  title: string;
  value: number;
  presets: Options<number>;
  limit: NumberLimit;
  unit?: string;
  onCommit: (value: number) => void;
}) {
  return (
    <SettingChip label={label} value={String(value)} unit={unit} title={title}>
      {() => (
        <ChipField label={title}>
          <Segmented
            label={`${title} presets`}
            value={value}
            options={presets}
            onChange={onCommit}
          />
          <NumberField
            className="min-w-0 flex-1"
            label={title}
            value={value}
            unit={unit}
            {...limit}
            onCommit={onCommit}
          />
        </ChipField>
      )}
    </SettingChip>
  );
}

export function serviceOptions(
  services: readonly { id: number; label: string }[],
  value: number | null,
): Options<string> {
  const options = [
    { value: "", label: "Auto" },
    ...services.map((service) => ({
      value: String(service.id),
      label: service.label || String(service.id),
    })),
  ];
  if (value !== null && !services.some((service) => service.id === value)) {
    options.push({ value: String(value), label: String(value) });
  }
  return options;
}

function ServiceChip({
  status,
  value,
  max,
  onChange,
}: {
  status?: BroadcastStatus;
  value: number | null;
  max: number;
  onChange: (id: number | null) => void;
}) {
  const services = status?.services ?? [];
  if (services.length === 0) {
    return (
      <OptionalNumberChip
        label="Service"
        title={SERVICE_TITLE}
        placeholder="Auto"
        value={value}
        min={0}
        max={max}
        step={1}
        onCommit={onChange}
      />
    );
  }
  return (
    <ChoiceChip
      label="Service"
      title={SERVICE_TITLE}
      value={value === null ? "" : String(value)}
      options={serviceOptions(services, value)}
      quiet={value === null}
      onChange={(id) => onChange(id === "" ? null : Number(id))}
    />
  );
}
