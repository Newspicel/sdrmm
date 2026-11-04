import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { HealthStats, hasHealth, healthStats } from "./Health";

describe("health footer", () => {
  it("shows only the values the decoder set", () => {
    expect(healthStats(undefined)).toEqual([]);
    expect(healthStats({})).toEqual([]);
    expect(healthStats({ snr_db: 12.34 }).map((stat) => stat.text)).toEqual(["12.3 dB"]);
    expect(hasHealth({})).toBe(false);
    expect(hasHealth({ lock: false })).toBe(true);
  });

  it("formats offset and error rate, warning on errors", () => {
    const stats = healthStats({ freq_error_hz: -1500, errors_per_s: 2 });
    expect(stats.map((stat) => [stat.label, stat.text, stat.tone])).toEqual([
      ["Δf", "−1.5 kHz", undefined],
      ["Err", "2.0/s", "warn"],
    ]);
    expect(healthStats({ errors_per_s: 0 })[0]?.tone).toBeUndefined();
  });

  it("draws a lock dot titled by its state", () => {
    const locked = renderToStaticMarkup(<HealthStats health={{ lock: true, snr_db: 9 }} />);
    expect(locked).toContain('title="Locked"');
    expect(locked).toContain("bg-ok");
    expect(locked).toContain('title="Signal to noise ratio"');
    const lost = renderToStaticMarkup(<HealthStats health={{ lock: false }} />);
    expect(lost).toContain('aria-label="No lock"');
    expect(lost).not.toContain("SNR");
  });
});
