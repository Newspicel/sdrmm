import { useQuery } from "@tanstack/react-query";
import { Fragment, type ReactNode, type RefObject, useCallback, useMemo, useRef } from "react";
import { Button } from "../../components/BaseControls";
import { identify, suggestedAt } from "../../components/bandPlan";
import { channelFrequencyHz, paramBandwidthHz } from "../../components/channelSettings";
import { plotButton } from "../../components/controls";
import { clampWindow } from "../../components/dbRange";
import { formatHz, formatMhz } from "../../components/format";
import type { DbWindow } from "../../components/spectrumTraces";
import {
  clusterMarkers,
  FULL_VIEW,
  isFullView,
  labelWidth,
  offsetToSpan,
  type SpectrumView,
  spanToOffset,
  spanToView,
  viewToSpan,
  viewWidth,
  wheelView,
} from "../../components/spectrumView";
import { GRAPHICS_HELP } from "../../gl/waterfall";
import { bookmarksQuery } from "../../lib/api";
import { heardHz, type SetLevels, useLevelStore } from "../../lib/levels";
import type { Bookmark, ChannelInfo, DeviceSet, PatchNode } from "../../lib/types";
import { useBandPlan } from "../../lib/useBandPlan";
import { useWorkspaceContext } from "../context";
import { type IqLane, iqSourceOf } from "../lanes";
import { channelPicker } from "../palette";
import { runningSetOf } from "../workspaceDevice";
import { BAND_RULER_H, BandRuler } from "./BandRuler";
import { ChannelPicker } from "./ChannelPicker";
import type { TrunkChannelOwner } from "./dmrTrunk";
import { FaceBody, NodeShell, useFaceActive, useFaceWheel } from "./NodeShell";
import { ScopeMenu } from "./ScopeMenu";
import { ScopeSettings } from "./ScopeSettings";
import { type ScopeChannels, useScopeChannels } from "./scopeChannels";
import { type ScopeDisplay, useScopeDisplay } from "./scopeDisplay";
import {
  displayWindow,
  type FrameMeta,
  type ScopeFeed,
  useScopeFeed,
  type WaterfallBox,
} from "./scopeFeed";
import { onPlotSurface, type ScopePreview, useScopeGesture } from "./scopeGesture";
import { bookmarkDraft, channelTypeAt, streamChannels } from "./scopePick";
import { useScopePicks } from "./scopePicks";
import type { DensityLayer } from "./scopePlot";

const NO_CHANNELS: readonly ChannelInfo[] = [];

const TRACE_MIN = 0.15;
const TRACE_MAX = 0.75;
const LABEL_TOP_PX = 28;

export function ScopeFace({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const set = runningSetOf(workspace, node.id);
  const source = iqSourceOf(workspace.resolved, node.id);
  return (
    <NodeShell node={node} title="Scope" category="output">
      <FaceBody scroll={false}>
        <Spectrum
          key={`${set?.id ?? "none"}:${source?.stream ?? 0}`}
          node={node.id}
          set={set}
          source={source}
        />
      </FaceBody>
    </NodeShell>
  );
}

function Spectrum({
  node,
  set,
  source,
}: {
  node: string;
  set: DeviceSet | null;
  source: IqLane | null;
}) {
  const workspace = useWorkspaceContext();
  const setId = set?.id ?? null;
  const stream = source?.stream ?? 0;
  const levels = useLevelStore((state) => (setId === null ? undefined : state.byDeviceSet[setId]));
  const channels = useMemo(
    () => streamChannels(set?.channels ?? NO_CHANNELS, stream),
    [set?.channels, stream],
  );
  const active = useFaceActive();
  const { plan, ruler: bandRuler, setRuler } = useBandPlan();
  const bookmarks = useQuery(bookmarksQuery());
  const plotRef = useRef<HTMLDivElement>(null);
  const traceRef = useRef<HTMLCanvasElement>(null);
  const waterfallRef = useRef<HTMLCanvasElement>(null);
  const hoverRef = useRef<number | null>(null);
  const densityRef = useRef<DensityLayer | null>(null);

  const display = useScopeDisplay();
  const feed = useScopeFeed({
    node,
    set,
    source,
    active,
    display,
    traceRef,
    waterfallRef,
    hoverRef,
    densityRef,
  });
  const tuning = useScopeChannels({ node, set, source, channels, meta: feed.meta });
  const gesture = useScopeGesture({
    plotRef,
    active,
    meta: feed.meta,
    channels,
    levels,
    display,
    hoverRef,
    tuning,
  });

  const picks = useScopePicks({
    plotRef,
    active,
    meta: feed.meta,
    view: display.view,
    pointerFraction: gesture.pointerFraction,
  });

  const meta = feed.meta;
  const view = display.view;
  const setView = display.setView;

  const applyRange = (next: DbWindow | null): void => {
    display.holdRange(next);
    feed.scheduleReseed();
    densityRef.current?.clear();
  };

  useFaceWheel(
    useCallback(
      (event: WheelEvent): boolean => {
        const plot = plotRef.current;
        if (plot === null || !wheelOnPlot(event, plot, active)) {
          return false;
        }
        const rect = plot.getBoundingClientRect();
        const at = (event.clientX - rect.left) / rect.width;
        setView((current) => wheelView(current, event, at, rect.width));
        return true;
      },
      [active, setView],
    ),
  );

  const rulerH = bandRuler && plan !== null ? BAND_RULER_H : 0;
  const openMenu = picks.menu;
  const openPicker = picks.picker;
  const suggestedType = (hz: number): string =>
    channelTypeAt(
      plan === null ? null : suggestedAt(identify(plan, hz)),
      channels.find((channel) => channel.id === tuning.tunableChannel),
    );

  const onDoubleClick = (event: React.MouseEvent<HTMLDivElement>): void => {
    const plot = plotRef.current;
    if (!active || meta === null || plot === null || !onPlotSurface(event.target, plot)) {
      return;
    }
    const at = gesture.pointerFraction(event.clientX);
    tuning.tuneCenter(Math.round(meta.centerHz + spanToOffset(viewToSpan(view, at), meta.spanHz)));
    setView(FULL_VIEW);
  };

  return (
    <div
      ref={plotRef}
      className={`relative flex h-full min-h-0 flex-col overflow-hidden bg-plot-bg ${
        active ? "nodrag nopan touch-none cursor-crosshair" : "cursor-default"
      } ${gesture.panning ? "!cursor-grabbing" : ""}`}
      onPointerDown={gesture.onPointerDown}
      onPointerMove={gesture.onPointerMove}
      onPointerUp={gesture.onPointerUp}
      onPointerCancel={gesture.onPointerUp}
      onPointerLeave={() => gesture.hover(null)}
      onContextMenu={picks.onContextMenu}
      onDoubleClick={onDoubleClick}
    >
      {meta !== null && (
        <BandRuler
          centerHz={meta.centerHz}
          spanHz={meta.spanHz}
          view={view}
          onTune={tuning.onBandTune}
        />
      )}
      <canvas
        ref={traceRef}
        className="w-full shrink-0"
        style={{ height: `${display.traceFraction * 100}%` }}
      />
      <Divider
        fraction={display.traceFraction}
        onFraction={display.setTraceFraction}
        plotRef={plotRef}
      />
      <canvas ref={waterfallRef} className="w-full min-h-0 flex-1" />

      <ScopeOverlays
        meta={meta}
        view={view}
        bookmarks={bookmarks.data ?? []}
        channels={channels}
        levels={levels}
        tuning={tuning}
        preview={gesture.preview}
        labelTop={rulerH + LABEL_TOP_PX}
        waterfall={feed.waterfall}
      />

      <ScopeChrome
        display={display}
        feed={feed}
        active={active}
        hovering={gesture.hovering}
        rulerH={rulerH}
        bands={bandRuler}
        onBands={() => setRuler(!bandRuler)}
        onRange={applyRange}
      />

      {openMenu !== null && (
        <ScopeMenu
          pick={openMenu.pick}
          at={openMenu.at}
          draft={bookmarkDraft(openMenu.pick.hz, plan)}
          onTune={() => {
            tuning.tuneTo(openMenu.pick);
            picks.closeMenu();
          }}
          onChannel={picks.pickChannel}
          onClose={picks.closeMenu}
        />
      )}

      {openPicker !== null && (
        <ChannelPicker
          title="New channel"
          note={formatHz(openPicker.hz)}
          groups={channelPicker(workspace.context.channelTypes, suggestedType(openPicker.hz))}
          onChannel={(channelType) => {
            tuning.addChannelAt(openPicker, channelType);
            picks.closePicker();
          }}
          onClose={picks.closePicker}
        />
      )}

      <GraphicsFault error={feed.glError} />
    </div>
  );
}

function GraphicsFault({ error }: { error: string | null }) {
  if (error === null) {
    return null;
  }
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center p-2">
      <span
        className="pointer-events-auto rounded-control border border-danger bg-bg/85 px-2 py-1 font-mono text-xs text-danger"
        title={GRAPHICS_HELP}
      >
        waterfall unavailable: {error}
      </span>
    </div>
  );
}

function ScopeOverlays({
  meta,
  view,
  bookmarks,
  channels,
  levels,
  tuning,
  preview,
  labelTop,
  waterfall,
}: {
  meta: FrameMeta | null;
  view: SpectrumView;
  bookmarks: readonly Bookmark[];
  channels: readonly ChannelInfo[];
  levels: SetLevels | undefined;
  tuning: ScopeChannels;
  preview: ScopePreview | null;
  labelTop: number;
  waterfall: WaterfallBox;
}) {
  if (meta === null) {
    return null;
  }
  return (
    <>
      <Bookmarks
        bookmarks={bookmarks}
        centerHz={meta.centerHz}
        spanHz={meta.spanHz}
        view={view}
        labelTop={waterfall.top + 3}
        widthPx={waterfall.width}
      />
      <Markers
        channels={channels}
        levels={levels}
        view={view}
        centerHz={meta.centerHz}
        spanHz={meta.spanHz}
        selected={tuning.selectedChannel}
        owners={tuning.owners}
        locked={tuning.locked}
        preview={preview}
        onSelect={tuning.selectChannel}
        labelTop={labelTop}
        widthPx={waterfall.width}
      />
    </>
  );
}

function ScopeChrome({
  display,
  feed,
  active,
  hovering,
  rulerH,
  bands,
  onBands,
  onRange,
}: {
  display: ScopeDisplay;
  feed: ScopeFeed;
  active: boolean;
  hovering: boolean;
  rulerH: number;
  bands: boolean;
  onBands: () => void;
  onRange: (next: DbWindow | null) => void;
}) {
  const meta = feed.meta;
  const view = display.view;
  const togglePhosphor = (): void => {
    if (!display.phosphor && display.range === null) {
      onRange(clampWindow(displayWindow(meta, null)));
    }
    display.setPhosphor(!display.phosphor);
  };
  return (
    <div
      className="pointer-events-none absolute inset-x-0 bottom-0 flex flex-col justify-between p-1.5"
      style={{ top: rulerH }}
    >
      <span
        className={`legend self-end ${hovering && active ? "invisible" : ""} font-mono text-2xs text-right whitespace-pre text-plot-ink-dim`}
      >
        {meta !== null && `${formatCentre(meta, view)}${formatFloor(meta.floorDb)}`}
        {display.range !== null && " · manual"}
      </span>
      <div
        data-plot-chrome
        className="pointer-events-auto flex items-center gap-1 self-start rounded-control bg-plot-bg/85 p-0.5"
      >
        <ScopeSettings
          colormap={display.paletteChoice}
          onColormap={display.chooseColormap}
          custom={display.customStops}
          onCustom={display.chooseCustom}
          average={display.average}
          onAverage={display.chooseAverage}
          traces={display.traceModes}
          onTrace={display.toggleTrace}
          phosphor={display.phosphor}
          onPhosphor={togglePhosphor}
          bands={bands}
          onBands={onBands}
          range={clampWindow(displayWindow(meta, display.range))}
          manual={display.range !== null}
          onRange={onRange}
          onAuto={() => onRange(null)}
        />
        {!isFullView(view) && (
          <Button
            type="button"
            className={plotButton(false)}
            onClick={() => display.setView(FULL_VIEW)}
          >
            {(1 / viewWidth(view)).toFixed(1)}× · reset
          </Button>
        )}
      </div>
    </div>
  );
}

function Divider({
  fraction,
  onFraction,
  plotRef,
}: {
  fraction: number;
  onFraction: (fraction: number) => void;
  plotRef: RefObject<HTMLDivElement | null>;
}) {
  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label={`Trace and waterfall split, ${Math.round(fraction * 100)}% trace`}
      data-plot-chrome
      className="group relative z-10 -my-1 h-[9px] shrink-0 cursor-row-resize"
      onPointerDown={(event) => {
        event.stopPropagation();
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (!event.currentTarget.hasPointerCapture(event.pointerId) || plotRef.current === null) {
          return;
        }
        event.stopPropagation();
        const rect = plotRef.current.getBoundingClientRect();
        const next = (event.clientY - rect.top) / rect.height;
        onFraction(Math.min(TRACE_MAX, Math.max(TRACE_MIN, next)));
      }}
      onPointerUp={(event) => {
        event.stopPropagation();
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
    >
      <span
        aria-hidden
        className="absolute inset-x-0 top-1 h-px bg-plot-ink-dim/40 group-hover:bg-accent"
      />
    </div>
  );
}

function Markers({
  channels,
  levels,
  view,
  centerHz,
  spanHz,
  selected,
  owners,
  locked,
  preview,
  onSelect,
  labelTop,
  widthPx,
}: {
  channels: readonly ChannelInfo[];
  levels: SetLevels | undefined;
  view: SpectrumView;
  centerHz: number;
  spanHz: number;
  selected: number | null;
  owners: ReadonlyMap<number, TrunkChannelOwner>;
  locked: ReadonlyMap<number, string>;
  preview: { channel: number; offsetHz: number } | null;
  onSelect: (channel: number) => void;
  labelTop: number;
  widthPx: number;
}) {
  const held = (channel: number): boolean => owners.has(channel) || locked.has(channel);
  const visible = spanHz * viewWidth(view);
  const drawn = channels
    .map((channel) => {
      const offsetHz =
        preview?.channel === channel.id
          ? preview.offsetHz
          : heardHz(channelFrequencyHz(channel.settings), levels?.[channel.id]) - centerHz;
      return {
        channel,
        hz: centerHz + offsetHz,
        id: channel.id,
        at: spanToView(view, offsetToSpan(offsetHz, spanHz)),
        width: labelWidth(markerName(channel, owners.get(channel.id)), widthPx),
      };
    })
    .filter((marker) => marker.at >= -0.02 && marker.at <= 1.02);
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {clusterMarkers(drawn).map((members) => {
        const anchor = members[0];
        if (anchor === undefined) {
          return null;
        }
        const shown = members.find((member) => member.channel.id === selected) ?? anchor;
        const stacked = members.length > 1;
        return (
          <div key={anchor.channel.id}>
            {members.map(({ channel, hz, at }) => {
              const active = channel.id === selected;
              const owner = owners.get(channel.id);
              const bandwidth = paramBandwidthHz(channel.settings.params);
              return (
                <Fragment key={channel.id}>
                  {bandwidth !== null && visible > 0 && (
                    <span
                      aria-hidden
                      className={`absolute inset-y-0 -translate-x-1/2 ${active ? "bg-plot-ink/10" : "bg-plot-ink/5"}`}
                      style={{ left: `${at * 100}%`, width: `${(bandwidth / visible) * 100}%` }}
                    />
                  )}
                  <span
                    aria-hidden
                    className={`absolute inset-y-0 -translate-x-1/2 ${
                      active ? "w-0.5 bg-plot-ink" : "w-px bg-plot-ink-dim"
                    }`}
                    style={{ left: `${at * 100}%` }}
                  />
                  <Button
                    type="button"
                    data-marker={channel.id}
                    className={`pointer-events-auto absolute inset-y-0 w-6 -translate-x-1/2 ${
                      held(channel.id) ? "cursor-pointer" : "cursor-ew-resize"
                    }`}
                    style={{ left: `${at * 100}%` }}
                    onClick={(event) => {
                      event.stopPropagation();
                      onSelect(channel.id);
                    }}
                    aria-label={markerHint(channel, hz, owner, locked.get(channel.id))}
                  />
                </Fragment>
              );
            })}

            <div
              className="pointer-events-auto group absolute flex -translate-x-1/2 flex-col items-center gap-1"
              style={{ left: `${shown.at * 100}%`, top: labelTop }}
            >
              <MarkerLabel
                active={shown.channel.id === selected}
                channel={shown.channel.id}
                owned={held(shown.channel.id)}
                title={markerHint(
                  shown.channel,
                  shown.hz,
                  owners.get(shown.channel.id),
                  locked.get(shown.channel.id),
                )}
                className={stacked ? "group-hover:hidden" : ""}
              >
                {markerName(shown.channel, owners.get(shown.channel.id))}
                {stacked && <span className="ml-1 text-plot-ink-dim">×{members.length}</span>}
              </MarkerLabel>
              {stacked &&
                members.map(({ channel, hz }) => (
                  <MarkerLabel
                    key={channel.id}
                    active={channel.id === selected}
                    channel={channel.id}
                    owned={held(channel.id)}
                    title={markerHint(channel, hz, owners.get(channel.id), locked.get(channel.id))}
                    className="hidden group-hover:block"
                  >
                    {markerName(channel, owners.get(channel.id))}
                  </MarkerLabel>
                ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function Bookmarks({
  bookmarks,
  centerHz,
  spanHz,
  view,
  labelTop,
  widthPx,
}: {
  bookmarks: readonly Bookmark[];
  centerHz: number;
  spanHz: number;
  view: SpectrumView;
  labelTop: number;
  widthPx: number;
}) {
  if (!(spanHz > 0)) {
    return null;
  }
  const drawn = bookmarks
    .map((bookmark) => ({
      bookmark,
      at: spanToView(view, offsetToSpan(bookmark.freq_hz - centerHz, spanHz)),
      width: labelWidth(bookmark.label, widthPx),
    }))
    .filter((mark) => mark.at >= 0 && mark.at <= 1);
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {drawn.map(({ bookmark, at }) => (
        <span
          key={bookmark.id}
          aria-hidden
          className="absolute inset-y-0 w-0 border-l border-dashed border-accent/40"
          style={{ left: `${at * 100}%` }}
        />
      ))}
      {clusterMarkers(drawn).map((members) => {
        const anchor = members[0];
        if (anchor === undefined) {
          return null;
        }
        return (
          <span
            key={anchor.bookmark.id}
            title={members
              .map((mark) => `${mark.bookmark.label}: ${formatMhz(mark.bookmark.freq_hz)}`)
              .join("\n")}
            className="absolute -translate-x-1/2 rounded-inset border border-accent/40 bg-bg/85 px-1 py-px font-mono text-2xs whitespace-nowrap text-accent"
            style={{ left: `${anchor.at * 100}%`, top: `${labelTop}px` }}
          >
            {anchor.bookmark.label}
            {members.length > 1 && <span className="ml-1 text-ink-dim">×{members.length}</span>}
          </span>
        );
      })}
    </div>
  );
}

function markerName(channel: ChannelInfo, owner: TrunkChannelOwner | undefined): string {
  return (owner?.role ?? channel.settings.params.type).toUpperCase();
}

function markerHint(
  channel: ChannelInfo,
  hz: number,
  owner: TrunkChannelOwner | undefined,
  lock: string | undefined,
): string {
  if (owner !== undefined) {
    return `trunk ${owner.role} channel: ${formatMhz(hz)}; the system it belongs to tunes it`;
  }
  if (lock !== undefined) {
    return `${channel.settings.params.type} channel: ${formatMhz(hz)}; ${lock}`;
  }
  return `${channel.settings.params.type} channel: ${formatMhz(hz)}`;
}

function MarkerLabel({
  active,
  className,
  channel,
  owned = false,
  title,
  children,
}: {
  active: boolean;
  className?: string;
  channel?: number;
  owned?: boolean;
  title?: string;
  children: ReactNode;
}) {
  return (
    <span
      aria-hidden
      data-marker={channel}
      title={title}
      className={`rounded-inset border px-1 py-px font-mono text-2xs whitespace-nowrap tabular-nums ${
        active ? "border-accent bg-bg text-accent" : "border-line bg-bg/85 text-ink-dim"
      } ${channel === undefined || owned ? "" : "cursor-ew-resize"} ${className ?? ""}`}
    >
      {children}
    </span>
  );
}

function wheelOnPlot(event: WheelEvent, plot: HTMLElement, active: boolean): boolean {
  if (active) {
    return event.target instanceof Node && plot.contains(event.target);
  }
  const rect = plot.getBoundingClientRect();
  return (
    event.clientX >= rect.left &&
    event.clientX < rect.right &&
    event.clientY >= rect.top &&
    event.clientY < rect.bottom
  );
}

function formatCentre(meta: FrameMeta, view: SpectrumView): string {
  const visible = meta.spanHz * viewWidth(view);
  const centre = meta.centerHz + spanToOffset((view.start + view.end) / 2, meta.spanHz);
  const span =
    visible >= 1e6 ? `${(visible / 1e6).toFixed(3)} MHz` : `${(visible / 1e3).toFixed(1)} kHz`;
  return `${(centre / 1e6).toFixed(4)} MHz   ${span}`;
}

function formatFloor(floorDb: number): string {
  return Number.isFinite(floorDb) ? `   floor ${floorDb.toFixed(0)} dBFS` : "";
}
