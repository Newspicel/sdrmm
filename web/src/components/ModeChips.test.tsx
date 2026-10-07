import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ChannelParams } from "../lib/types";
import { loraKeysShown } from "./LoraKeysChip";
import { bandwidthOptions, ModeChips, serviceOptions } from "./ModeChips";
import {
  blankLoraKey,
  LORA_CUSTOM,
  loraKeyComplete,
  loraPresetOf,
  withLoraKeyText,
  withLoraPreset,
} from "./modeOptions";

function render(params: ChannelParams): string {
  return renderToStaticMarkup(<ModeChips params={params} limits={[]} onParams={() => undefined} />);
}

describe("ModeChips", () => {
  it("shows the CTCSS chip only for CTCSS tone squelch", () => {
    expect(render({ type: "nfm", settings: {} })).not.toContain('aria-label="CTCSS tone"');
    const html = render({ type: "nfm", settings: { tone_mode: "ctcss", ctcss_hz: 100 } });
    expect(html).toContain('aria-label="CTCSS tone"');
    expect(html).toContain("100.0 Hz");
  });

  it("names the bandwidth chip for e2e", () => {
    expect(render({ type: "nfm", settings: {} })).toContain('aria-label="Channel bandwidth"');
  });

  it("offers the AM sync detector", () => {
    expect(render({ type: "am", settings: {} })).toContain("Sync");
  });

  it("swaps DVB-S code rate for S2 settings", () => {
    expect(render({ type: "datv", settings: {} })).toContain('aria-label="DVB-S code rate"');
    const s2 = render({ type: "datv", settings: { standard: "dvb_s2" } });
    expect(s2).not.toContain('aria-label="DVB-S code rate"');
    expect(s2).toContain("Roll-off");
  });

  it("offers Auto for the radiosonde type", () => {
    expect(render({ type: "radiosonde", settings: {} })).toContain("Auto");
    expect(render({ type: "radiosonde", settings: { sonde: "dfm" } })).toContain("DFM");
  });

  it("shows WEFAX timing and LRPT mode chips", () => {
    const wefax = render({ type: "wefax", settings: {} });
    expect(wefax).toContain('aria-label="Index of cooperation"');
    expect(wefax).toContain('aria-label="Lines per minute"');
    expect(render({ type: "lrpt", settings: {} })).toContain("OQPSK 72k");
  });

  it("offers the SSTV modulation", () => {
    const sstv = render({ type: "sstv", settings: {} });
    expect(sstv).toContain('aria-label="Modulation"');
    expect(sstv).toContain("USB");
    expect(render({ type: "sstv", settings: { modulation: "fm" } })).toContain("FM");
  });

  it("shows the detected DAB mode next to Auto", () => {
    const params: ChannelParams = { type: "dab", settings: { transmission_mode: "auto" } };
    expect(render(params)).toContain("Auto");
    const html = renderToStaticMarkup(
      <ModeChips
        params={params}
        broadcast={{
          system: "dab",
          locked: true,
          snr_db: 12,
          frequency_error_hz: 0,
          transmission_mode: "ii",
        }}
        limits={[]}
        onParams={() => undefined}
      />,
    );
    expect(html).toContain("Auto II");
  });

  it("offers a service picker for DRM", () => {
    expect(render({ type: "drm", settings: {} })).toContain("Service");
  });

  it("shows the LoRa modem, keys and a matching preset", () => {
    const html = render({ type: "lora", settings: {} });
    expect(html).toContain('aria-label="LoRa bandwidth"');
    expect(html).toContain("125 kHz");
    expect(html).toContain('aria-label="Spreading factor"');
    expect(html).toContain("built-in");
    expect(html).toContain("Custom");
    expect(html).not.toContain('aria-label="Implicit header coding rate"');
    const fast = render({
      type: "lora",
      settings: { bandwidth: "khz250", spreading_factor: "sf11", protocol: "meshtastic" },
    });
    expect(fast).toContain("LongFast");
  });

  it("adds coding rate and CRC chips for an implicit header", () => {
    const html = render({
      type: "lora",
      settings: { implicit_header: { length: 12, coding_rate: "4/8", crc: false } },
    });
    expect(html).toContain('aria-label="Implicit header coding rate"');
    expect(html).toContain("4/8");
    expect(html).toContain('aria-label="Implicit header payload carries a CRC"');
  });

  it("renders nothing for modes without settings", () => {
    expect(render({ type: "m17", settings: {} })).toBe("");
  });
});

describe("bandwidthOptions", () => {
  it("keeps an off-list value as current", () => {
    const options = bandwidthOptions(15_000, [12_500, 25_000]);
    expect(options[0]).toEqual({ value: 15_000, label: "15 kHz (current)" });
    expect(options).toHaveLength(3);
  });

  it("matches numeric values", () => {
    expect(bandwidthOptions(12_500, [12_500, 25_000]).map((o) => o.value)).toEqual([
      12_500, 25_000,
    ]);
  });
});

describe("serviceOptions", () => {
  const status = [{ id: 7, label: "News" }];

  it("lists discovered services after Auto", () => {
    expect(serviceOptions(status, null)).toEqual([
      { value: "", label: "Auto" },
      { value: "7", label: "News" },
    ]);
  });

  it("keeps an unknown chosen service", () => {
    expect(serviceOptions(status, 9).at(-1)).toEqual({ value: "9", label: "9" });
  });
});

describe("LoRa presets", () => {
  it("sets modem and protocol but keeps keys", () => {
    const keys = [blankLoraKey("lorawan_app_key")];
    const next = withLoraPreset({ keys }, "meshcore_EU");
    expect(next).toEqual({
      keys,
      bandwidth: "khz62_5",
      spreading_factor: "sf8",
      protocol: "meshcore",
      iq: "normal",
    });
    expect(loraPresetOf(next)).toBe("meshcore_EU");
    expect(loraPresetOf({ ...next, iq: "both" })).toBe(LORA_CUSTOM);
    expect(withLoraPreset(next, "unknown")).toBe(next);
  });
});

describe("LoRa keys", () => {
  it("counts only what was added", () => {
    expect(loraKeysShown([])).toBe("built-in");
    expect(loraKeysShown([blankLoraKey("meshtastic_channel")])).toBe("+1");
  });

  it("needs the fields the decoder cannot do without", () => {
    const open = withLoraKeyText(blankLoraKey("meshtastic_channel"), "name", "Hike");
    expect(loraKeyComplete(open)).toBe(true);
    const tag = blankLoraKey("meshcore_channel");
    expect(loraKeyComplete(withLoraKeyText(tag, "name", "#"))).toBe(false);
    expect(loraKeyComplete(withLoraKeyText(tag, "name", "#ham"))).toBe(true);
    expect(loraKeyComplete(withLoraKeyText(tag, "name", "Club"))).toBe(false);
    const session = withLoraKeyText(blankLoraKey("lorawan_session"), "dev_addr", "26011BDA");
    expect(loraKeyComplete(session)).toBe(false);
    expect(
      loraKeyComplete(
        withLoraKeyText(withLoraKeyText(session, "nwk_s_key", "00"), "app_s_key", "11"),
      ),
    ).toBe(true);
  });
});
