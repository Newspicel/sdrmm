import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { RadioActivity } from "../../lib/types";
import { deviceSet } from "../../test/fixtures";
import { activitySignal } from "./activity";
import { DeviceStatus } from "./DeviceFace";

function header(activity: RadioActivity): string {
  return renderToStaticMarkup(<DeviceStatus set={deviceSet({ activity })} />);
}

describe("activitySignal", () => {
  it("lights the dot red while sending and amber while receive pauses for it", () => {
    expect(activitySignal("sending")?.tone).toBe("danger");
    expect(activitySignal("receive_paused")).toEqual({
      tone: "warn",
      label: "Sending, receive paused",
    });
    expect(activitySignal("receiving")?.tone).toBe("ok");
    expect(activitySignal("idle")).toBeNull();
  });
});

describe("DeviceStatus", () => {
  it("counts the decoders it hears while listening", () => {
    expect(header("receiving")).toContain("0/0");
    expect(header("receive_paused")).toContain("0/0");
  });

  it("says nothing while it does not listen", () => {
    expect(header("sending")).toBe("");
    expect(header("idle")).toBe("");
  });
});
