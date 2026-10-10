import { type Node, useViewport, ViewportPortal } from "@xyflow/react";
import { Fragment } from "react";
import { peerColor, useOthers, usePresenceStore } from "../lib/presence";
import type { Peer, Position } from "../lib/types";

const OUTLINE_GAP = 4;

export function PeerLayer({ nodes }: { nodes: readonly Node[] }) {
  const others = useOthers();
  const pointers = usePresenceStore((state) => state.pointers);
  const { zoom } = useViewport();
  return (
    <ViewportPortal>
      {others.map((peer) => {
        const pointer = pointers[peer.id];
        if (pointer === undefined) {
          return null;
        }
        return (
          <Fragment key={peer.id}>
            {(pointer.selected ?? []).map((id) => {
              const node = nodes.find((candidate) => candidate.id === id);
              return node === undefined ? null : (
                <Outline key={id} node={node} peer={peer} zoom={zoom} />
              );
            })}
            {pointer.at != null && <Cursor at={pointer.at} peer={peer} zoom={zoom} />}
          </Fragment>
        );
      })}
    </ViewportPortal>
  );
}

function Outline({ node, peer, zoom }: { node: Node; peer: Peer; zoom: number }) {
  const width = node.measured?.width ?? node.width ?? 0;
  const height = node.measured?.height ?? node.height ?? 0;
  const color = peerColor(peer);
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute top-0 left-0 rounded-[6px] border-2"
      style={{
        transform: `translate(${node.position.x - OUTLINE_GAP}px, ${node.position.y - OUTLINE_GAP}px)`,
        width: width + OUTLINE_GAP * 2,
        height: height + OUTLINE_GAP * 2,
        borderColor: color,
      }}
    >
      <span
        className="absolute bottom-full left-0 mb-0.5 origin-bottom-left rounded-[3px] px-1 text-[10px] whitespace-nowrap text-bg"
        style={{ background: color, transform: `scale(${1 / zoom})` }}
      >
        {peer.name}
      </span>
    </div>
  );
}

function Cursor({ at, peer, zoom }: { at: Position; peer: Peer; zoom: number }) {
  const color = peerColor(peer);
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute top-0 left-0 origin-top-left transition-transform duration-75 ease-linear"
      style={{ transform: `translate(${at.x}px, ${at.y}px) scale(${1 / zoom})` }}
    >
      <svg width="16" height="18" viewBox="0 0 16 18" className="drop-shadow">
        <path
          d="M1 1 L1 15 L5 11 L8 17 L10 16 L7 10 L13 10 Z"
          fill={color}
          stroke="var(--color-bg)"
        />
      </svg>
      <span
        className="absolute top-4 left-3 rounded-[3px] px-1 text-[10px] whitespace-nowrap text-bg"
        style={{ background: color }}
      >
        {peer.name}
      </span>
    </div>
  );
}
