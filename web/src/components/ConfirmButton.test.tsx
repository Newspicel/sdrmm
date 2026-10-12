import { Trash2 } from "lucide-react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConfirmButton, confirmClass, confirmLabel } from "./ConfirmButton";
import { BTN_DANGER, BTN_DANGER_ARMED, BTN_DANGER_SM, ICON_BTN_SM_DANGER_ARMED } from "./controls";

describe("ConfirmButton", () => {
  it("asks to confirm the same action it names", () => {
    expect(confirmLabel("Clear")).toBe("Confirm clear");
    expect(confirmLabel("Clear all")).toBe("Confirm clear all");
  });

  it("reads as a danger action before and after arming, at every size", () => {
    expect(confirmClass("md", false)).toBe(BTN_DANGER);
    expect(confirmClass("md", true)).toBe(BTN_DANGER_ARMED);
    expect(confirmClass("sm", true)).toBe(BTN_DANGER_SM);
    expect(confirmClass("icon", true)).toBe(ICON_BTN_SM_DANGER_ARMED);
  });

  it("names an icon button by its action", () => {
    const html = renderToStaticMarkup(
      <ConfirmButton label="Delete dipole" icon={Trash2} size="icon" onConfirm={() => {}} />,
    );
    expect(html).toContain('aria-label="Delete dipole"');
    expect(html).toContain('title="Delete dipole"');
  });

  it("shows the action as text without an icon", () => {
    const html = renderToStaticMarkup(<ConfirmButton label="Clear" onConfirm={() => {}} />);
    expect(html).toContain(">Clear</button>");
  });
});
