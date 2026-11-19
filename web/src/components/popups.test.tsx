import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { InfoTip } from "./InfoTip";
import { Popover } from "./Popover";
import { Tip, tipRole } from "./Tip";

describe("tipRole", () => {
  it("names a trigger whose label it repeats", () => {
    expect(tipRole(<button type="button" aria-label="Mute" />, "Mute")).toBe("label");
  });

  it("leaves any other trigger to the default", () => {
    expect(tipRole(<button type="button" aria-label="Mute" />, "Silence the speaker")).toBe(
      undefined,
    );
    expect(tipRole(<button type="button" />, "Mute")).toBe(undefined);
  });
});

describe("Tip", () => {
  it("adds nothing to its trigger's row until it opens", () => {
    const html = renderToStaticMarkup(
      <span>
        <Tip text="Decibel full scale" render={<span />}>
          dBFS
        </Tip>
      </span>,
    );

    expect(html).not.toContain("Decibel full scale");
    expect(html).toMatch(/^<span><span[^>]*>dBFS<\/span><\/span>$/);
  });
});

describe("Popover", () => {
  it("draws only its trigger while closed", () => {
    const html = renderToStaticMarkup(
      <Popover label="Peers" triggerClass="chip">
        {() => "inside"}
      </Popover>,
    );

    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain("inside");
  });

  it("names a titled trigger with its tip", () => {
    const html = renderToStaticMarkup(
      <Popover label="P" triggerClass="chip" title="Peers">
        {() => "inside"}
      </Popover>,
    );

    expect(html).toContain('aria-label="Peers"');
    expect(html).toContain("aria-labelledby=");
    expect(html).not.toContain("inside");
  });
});

describe("InfoTip", () => {
  it("is a button named by its text, with the text kept until opened", () => {
    const html = renderToStaticMarkup(<InfoTip text="Gain before the mixer" />);

    expect(html).toContain('aria-label="Gain before the mixer"');
    expect(html).toContain('aria-expanded="false"');
    expect(html.match(/Gain before the mixer/g)).toHaveLength(1);
  });
});
