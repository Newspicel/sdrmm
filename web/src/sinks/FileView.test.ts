import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { FileView } from "./FileView";

describe("file view", () => {
  it("draws a picture and offers it for download", () => {
    const html = renderToStaticMarkup(
      createElement(FileView, {
        file: {
          name: "slide.png",
          media_type: "image/png",
          image: true,
          url: "data:image/png;base64,iVBORw==",
        },
      }),
    );
    expect(html).toContain('alt="slide.png"');
    expect(html).toContain('download="slide.png"');
  });

  it("only offers anything else for download", () => {
    const html = renderToStaticMarkup(
      createElement(FileView, {
        file: {
          name: ".._page.html",
          media_type: "text/html",
          url: "data:application/octet-stream;base64,PHNjcmlwdD4=",
        },
      }),
    );
    expect(html).toContain('download=".._page.html"');
    expect(html).not.toContain("<img");
  });
});
