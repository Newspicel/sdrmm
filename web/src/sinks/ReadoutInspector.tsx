import { Inspect, InspectSection } from "../canvas/inspector";
import { Button } from "../components/BaseControls";
import { ConfirmButton } from "../components/ConfirmButton";
import { BTN } from "../components/controls";
import { formatAge } from "../components/decoderViews";
import { Readout, Readouts } from "../components/face/Readouts";
import { copyText } from "../lib/copyText";
import { useEventKinds } from "../lib/eventKinds";
import { groupsOf, type SinkView, useSinkStore } from "../lib/sinks";
import { toastError } from "../lib/toasts";
import { readoutText, sourceRows } from "./readoutText";

/** What the readout's sidebar shows: each source's pace, what was lost, and copy and clear. */
export function ReadoutInspector({
  node,
  feed,
  sourceName,
  now,
}: {
  node: string;
  feed: SinkView;
  sourceName: (source: string) => string;
  now: number;
}) {
  const entries = useEventKinds();
  const groups = groupsOf(feed);
  const rows = sourceRows(groups, entries, sourceName);
  const text = readoutText(groups, entries, sourceName);
  const clear = useSinkStore((state) => state.clear);
  return (
    <Inspect node={node}>
      <InspectSection label="Sources">
        <Readouts padded={false} ruled={false}>
          {rows.length === 0 ? (
            <Readout label="Events">{null}</Readout>
          ) : (
            rows.map((row) => (
              <Readout key={row.key} label={row.label} title="Events shown, time since the last">
                {`${row.count} · ${row.lastMs === null ? "–" : formatAge(now - row.lastMs)}`}
              </Readout>
            ))
          )}
          <Readout
            label="Lost"
            title="Events dropped before they reached this node"
            tone={feed.lost > 0 ? "warn" : undefined}
          >
            {feed.lost}
          </Readout>
        </Readouts>
      </InspectSection>
      <InspectSection>
        <div className="flex gap-2">
          <Button
            type="button"
            className={BTN}
            disabled={text === ""}
            onClick={() => {
              copyText(text).catch(toastError);
            }}
          >
            Copy all
          </Button>
          <ConfirmButton
            label="Clear"
            size="sm"
            title="Empty this readout until new events arrive"
            onConfirm={() => clear(node)}
          />
        </div>
      </InspectSection>
    </Inspect>
  );
}
