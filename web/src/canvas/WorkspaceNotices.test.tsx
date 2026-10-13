import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { WorkspaceNotice } from "../lib/types";
import { type KindNames, NoticeList, noticeLine } from "./WorkspaceNotices";

const CATALOG: KindNames = { nodes: [{ kind: "scope", name: "Scope" }] };

function dropped(id: number, nodes: [string, string][]): WorkspaceNotice {
  return {
    id,
    at: "2026-09-28T12:00:00Z",
    kind: "dropped_nodes",
    data: { nodes: nodes.map(([node, kind]) => ({ id: node, kind })) },
  };
}

function clearedGps(id: number, nodes: string[]): WorkspaceNotice {
  return { id, at: "2026-09-28T12:00:00Z", kind: "cleared_gps", data: { nodes } };
}

type ButtonProps = { children?: ReactNode; disabled?: boolean; onClick?: () => void };

type Clickable = ReactElement<ButtonProps>;

function buttons(node: ReactNode): Clickable[] {
  if (Array.isArray(node)) {
    return node.flatMap(buttons);
  }
  if (!isValidElement<ButtonProps>(node)) {
    return [];
  }
  const own = node.props.children === "Dismiss" ? [node] : [];
  return [...own, ...buttons(node.props.children)];
}

describe("WorkspaceNotices", () => {
  it("lists removed kinds and dismisses", () => {
    const notices = [
      dropped(7, [
        ["df1", "df"],
        ["arr", "array"],
        ["radar", "passive_radar"],
      ]),
      clearedGps(8, ["gps1", "gps2"]),
    ];
    const lines = notices.map((notice) => noticeLine(notice, CATALOG));
    const onDismiss = vi.fn();

    const html = renderToStaticMarkup(
      <NoticeList lines={lines} dismissing={null} onDismiss={onDismiss} />,
    );

    expect(html).toContain("Removed old nodes: DF, Array, Passive radar");
    expect(html).toContain('title="df1, arr, radar"');
    expect(html).toContain("GPS source cleared: gps1, gps2");
    expect(html).toContain('title="This device is gone. Pick a phone."');

    const dismiss = buttons(NoticeList({ lines, dismissing: null, onDismiss }));
    expect(dismiss).toHaveLength(2);
    dismiss[1]?.props.onClick?.();
    expect(onDismiss).toHaveBeenCalledWith(8);
  });

  it("holds the button of a notice while it goes", () => {
    const lines = [dropped(7, [["df1", "df"]]), clearedGps(8, ["gps1"])].map((notice) =>
      noticeLine(notice, CATALOG),
    );
    const dismiss = buttons(NoticeList({ lines, dismissing: 7, onDismiss: () => undefined }));
    expect(dismiss.map((button) => button.props.disabled)).toEqual([true, false]);
  });

  it("names each kind once and counts the rest", () => {
    const line = noticeLine(
      dropped(1, [
        ["a", "df"],
        ["b", "df"],
        ["c", "array"],
        ["d", "old_mixer"],
        ["e", "stitch"],
        ["f", "passive_radar"],
      ]),
      CATALOG,
    );
    expect(line.text).toBe("Removed old nodes: DF, Array, Old mixer, Stitch +1");
  });

  it("prefers the catalog name of a kind", () => {
    const line = noticeLine(dropped(1, [["a", "df"]]), {
      nodes: [{ kind: "df", name: "Direction finder" }],
    });
    expect(line.text).toBe("Removed old nodes: Direction finder");
  });

  it("names the filter kinds it removed", () => {
    const line = noticeLine(
      {
        id: 3,
        at: "2026-09-28T12:00:00Z",
        kind: "dropped_filter_kinds",
        data: { node: "only", kinds: ["teletext", "fax"] },
      },
      CATALOG,
    );
    expect(line.text).toBe("Unknown filter kinds removed: teletext, fax");
    expect(line.title).toBe("only");
  });

  it("lists the wires a contract refused with their reasons", () => {
    const line = noticeLine(
      {
        id: 4,
        at: "2026-10-12T12:00:00Z",
        kind: "dropped_wires",
        data: {
          wires: [
            {
              from: { node: "pager", port: "events" },
              to: { node: "map", port: "events" },
              reason: "Map takes positions",
            },
          ],
        },
      },
      CATALOG,
    );
    expect(line.text).toBe("Removed 1 refused wire");
    expect(line.title).toBe("pager to map: Map takes positions");
  });

  it("lists the wires a step added", () => {
    const line = noticeLine(
      {
        id: 5,
        at: "2026-10-12T12:00:00Z",
        kind: "added_wires",
        data: {
          wires: [
            {
              from: { node: "dev", port: "iq2" },
              to: { node: "scan", port: "iq" },
              reason: "The lane it watches",
            },
          ],
        },
      },
      CATALOG,
    );
    expect(line.text).toBe("Added 1 wire");
    expect(line.title).toBe("dev to scan: The lane it watches");
  });

  it("names the nodes a step added", () => {
    const line = noticeLine(
      {
        id: 6,
        at: "2026-10-12T12:00:00Z",
        kind: "added_nodes",
        data: { nodes: [{ id: "sstv-pictures", kind: "video" }] },
      },
      { nodes: [{ kind: "video", name: "Video" }] },
    );
    expect(line.text).toBe("Added nodes: Video");
    expect(line.title).toBe("sstv-pictures");
  });

  it("shows nothing without notices", () => {
    expect(
      renderToStaticMarkup(<NoticeList lines={[]} dismissing={null} onDismiss={() => undefined} />),
    ).toBe("");
  });
});
