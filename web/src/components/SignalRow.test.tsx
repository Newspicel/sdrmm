import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SignalRow } from "./SignalRow";

const level = { channel: 1, level_db: -40, peak_db: -30, squelch_db: null };

function gated(open: boolean): string {
  return renderToStaticMarkup(
    <SignalRow
      level={{ ...level, open }}
      squelch={{ mode: "manual", level_db: -50 }}
      onSquelch={() => undefined}
    />,
  );
}

describe("SignalRow", () => {
  it("shows a plain meter when the decoder has no squelch", () => {
    const html = renderToStaticMarkup(<SignalRow level={level} />);
    expect(html).toContain('role="meter"');
    expect(html).toContain("-40.0 dB");
    expect(html).not.toContain("Squelch threshold");
  });

  it("puts the squelch handle and Auto on the meter", () => {
    const html = renderToStaticMarkup(
      <SignalRow
        level={level}
        squelch={{ mode: "auto", margin_db: 8 }}
        onSquelch={() => undefined}
      />,
    );
    expect(html).toContain('aria-label="Squelch threshold"');
    expect(html).toContain('aria-label="Automatic squelch"');
    expect(html).toContain('aria-pressed="true"');
  });

  it("fills the meter bright only while the gate the server reports is open", () => {
    expect(gated(true)).not.toContain("bg-accent-dim");
    expect(gated(false)).toContain("bg-accent-dim");
  });

  it("leaves Auto off for a manual squelch", () => {
    const html = renderToStaticMarkup(
      <SignalRow
        level={level}
        squelch={{ mode: "manual", level_db: -50 }}
        onSquelch={() => undefined}
      />,
    );
    expect(html).toContain('aria-pressed="false"');
  });
});
