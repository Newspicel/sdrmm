import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it } from "vitest";
import { useRefusalStore } from "../lib/refusals";
import { FaceAlert } from "./FaceAlert";

afterEach(() => useRefusalStore.getState().reset());

describe("FaceAlert", () => {
  it("renders nothing while the node has no refusal", () => {
    expect(renderToStaticMarkup(<FaceAlert node="arr" />)).toBe("");
  });

  it("shows the newest refusal as an icon with its text as the title", () => {
    useRefusalStore.getState().flag("arr", "that lane is in North", "action");
    const html = renderToStaticMarkup(<FaceAlert node="arr" />);
    expect(html).toContain('role="alert"');
    expect(html).toContain('aria-label="that lane is in North"');
    expect(html).not.toContain(">that lane is in North<");
  });
});
