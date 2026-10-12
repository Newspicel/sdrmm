import { CircleAlert, CircleCheck, CircleX, X } from "lucide-react";
import { Button } from "../../components/BaseControls";
import { ICON_BTN_SM, LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Icon } from "../../components/Icon";
import type { PointReadout } from "./analysis";
import type { BandFit } from "./bands";
import type { ActiveMarker, MarkerRow } from "./markers";
import { type LNetwork, lNetworks, type MatchElement } from "./matching";
import { formatDb, formatImpedance, formatNumber, formatSi, REFERENCE_OHMS } from "./nanovna";

interface Quality {
  label: string;
  tone: string;
  glyph: typeof CircleCheck;
}

export function matchQuality(swr: number): Quality {
  if (swr <= 1.5) {
    return { label: "Good match", tone: "text-ok", glyph: CircleCheck };
  }
  if (swr <= 2) {
    return { label: "Usable", tone: "text-ok", glyph: CircleCheck };
  }
  if (swr <= 3) {
    return { label: "Poor", tone: "text-warn", glyph: CircleAlert };
  }
  return { label: "Mismatch", tone: "text-danger", glyph: CircleX };
}

export function formatSwr(swr: number): string {
  if (!Number.isFinite(swr)) {
    return "∞";
  }
  return swr >= 100 ? swr.toFixed(0) : swr.toFixed(2);
}

export function AnalyzerReadout({ marker }: { marker: MarkerRow }) {
  const row = marker.row;
  const quality = matchQuality(row.vswr);
  const component = row.component;
  return (
    <section
      aria-label="Marker reading"
      className="flex flex-wrap items-end gap-x-8 gap-y-3 rounded-[3px] border border-line bg-panel-2/60 px-4 py-3"
    >
      <Big label={marker.id === "best" ? "Lowest SWR at" : `Marker ${marker.label}`}>
        {formatHz(row.frequencyHz)}
      </Big>
      <div className="flex flex-col gap-1">
        <span className={LABEL}>SWR</span>
        <span className="flex items-baseline gap-3">
          <span className="font-mono text-4xl leading-none font-semibold text-ink tabular-nums">
            {formatSwr(row.vswr)}
          </span>
          <span className={`inline-flex items-center gap-1 text-xs font-medium ${quality.tone}`}>
            <Icon glyph={quality.glyph} size={12} />
            {quality.label}
          </span>
        </span>
      </div>
      <Big label="Impedance">{formatImpedance(row.impedance)}</Big>
      <Small label="Return loss" value={formatDb(row.returnLossDb)} />
      <Small label="|Z|" value={`${formatNumber(row.impedanceMagnitude, 1)} Ω`} />
      <Small
        label={component?.kind === "inductance" ? "Series L" : "Series C"}
        value={
          component === null
            ? "-"
            : formatSi(component.value, component.kind === "inductance" ? "H" : "F", 2)
        }
      />
      <Small label="Q" value={formatNumber(row.q, 2)} />
      {Number.isFinite(row.s21Db) && <Small label="|S21|" value={formatDb(row.s21Db)} />}
    </section>
  );
}

function Big({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className={LABEL}>{label}</span>
      <span className="font-mono text-xl leading-none text-ink tabular-nums">{children}</span>
    </div>
  );
}

function Small({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className={LABEL}>{label}</span>
      <span className="font-mono text-sm leading-none text-ink-dim tabular-nums">{value}</span>
    </div>
  );
}

export function MarkerTable({
  markers,
  active,
  onActivate,
  onRemove,
}: {
  markers: readonly MarkerRow[];
  active: ActiveMarker;
  onActivate: (id: ActiveMarker) => void;
  onRemove: (id: number) => void;
}) {
  return (
    <Panel title="Markers" hint="Click the chart to place a marker. Arrow keys move it.">
      <table className="w-full font-mono text-xs tabular-nums">
        <thead>
          <tr className="text-left text-[10.5px] text-ink-faint">
            <th className="w-6 py-1 font-normal" />
            <th className="py-1 font-normal">Frequency</th>
            <th className="py-1 text-right font-normal">SWR</th>
            <th className="py-1 text-right font-normal">Z</th>
            <th className="w-6" />
          </tr>
        </thead>
        <tbody>
          {markers.map((marker) => (
            <tr
              key={marker.id}
              aria-selected={marker.id === active}
              onClick={() => onActivate(marker.id)}
              className={`cursor-pointer border-t border-line/60 ${
                marker.id === active ? "bg-accent/10 text-ink" : "text-ink-dim hover:bg-panel-2"
              }`}
            >
              <td className="py-1 pl-1 font-bold">{marker.label}</td>
              <td className="py-1">{formatHz(marker.row.frequencyHz)}</td>
              <td className="py-1 text-right">{formatSwr(marker.row.vswr)}</td>
              <td className="py-1 text-right">{formatImpedance(marker.row.impedance)}</td>
              <td className="text-right">
                {marker.id !== "best" && (
                  <Button
                    type="button"
                    className={ICON_BTN_SM}
                    aria-label={`Remove marker ${marker.label}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      onRemove(marker.id as number);
                    }}
                  >
                    <Icon glyph={X} size={12} />
                  </Button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

export function BandTable({
  fits,
  onFocus,
}: {
  fits: readonly BandFit[];
  onFocus: (fit: BandFit) => void;
}) {
  return (
    <Panel title="Bands" hint="Amateur bands inside the sweep. Click one to zoom in.">
      {fits.length === 0 ? (
        <p className="py-2 text-xs text-ink-faint">No amateur band in this range.</p>
      ) : (
        <table className="w-full font-mono text-xs tabular-nums">
          <thead>
            <tr className="text-left text-[10.5px] text-ink-faint">
              <th className="py-1 font-normal">Band</th>
              <th className="py-1 text-right font-normal">Best</th>
              <th className="py-1 text-right font-normal">At</th>
              <th className="py-1 text-right font-normal">Worst</th>
            </tr>
          </thead>
          <tbody>
            {fits.map((fit) => {
              const quality = matchQuality(fit.worstVswr);
              return (
                <tr
                  key={fit.band.label + fit.band.startHz}
                  onClick={() => onFocus(fit)}
                  className="cursor-pointer border-t border-line/60 text-ink-dim hover:bg-panel-2"
                >
                  <td className="py-1 text-ink">{fit.band.label}</td>
                  <td className="py-1 text-right">{formatSwr(fit.bestVswr)}</td>
                  <td className="py-1 text-right">{formatHz(fit.bestHz)}</td>
                  <td className={`py-1 text-right ${quality.tone}`}>
                    <span className="inline-flex items-center gap-1">
                      <Icon glyph={quality.glyph} size={12} />
                      {formatSwr(fit.worstVswr)}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </Panel>
  );
}

export function MatchPanel({ row }: { row: PointReadout }) {
  const networks = row.impedance === null ? [] : lNetworks(row.impedance, row.frequencyHz);
  return (
    <Panel
      title="Match to 50 Ω"
      hint="L networks that bring the marker impedance to 50 Ω. Shunt at load: the shunt part sits across the antenna."
    >
      {networks.length === 0 ? (
        <p className="py-2 text-xs text-ink-faint">
          {row.impedance === null ? "No impedance at this point." : "Already matched."}
        </p>
      ) : (
        <ul className="flex flex-col">
          {networks.map((network) => (
            <NetworkRow key={describe(network)} network={network} />
          ))}
        </ul>
      )}
      <p className="pt-1 font-mono text-[10.5px] text-ink-faint">
        {formatImpedance(row.impedance)} → {REFERENCE_OHMS} Ω at {formatHz(row.frequencyHz)}
      </p>
    </Panel>
  );
}

function NetworkRow({ network }: { network: LNetwork }) {
  const first = network.shuntAt === "load" ? "shunt" : "series";
  const parts =
    network.shuntAt === "load"
      ? [
          { role: "Shunt", element: network.shunt },
          { role: "Series", element: network.series },
        ]
      : [
          { role: "Series", element: network.series },
          { role: "Shunt", element: network.shunt },
        ];
  return (
    <li
      className="flex items-baseline gap-3 border-t border-line/60 py-1 font-mono text-xs tabular-nums"
      title={`Antenna, then ${first} element, then the radio`}
    >
      <span className="w-20 shrink-0 text-ink-faint">
        {network.shuntAt === "load" ? "At antenna" : "At radio"}
      </span>
      {parts.map((part) => (
        <span
          key={part.role}
          className="text-ink"
          title={`${formatNumber(part.element.reactanceOhms, 1)} Ω`}
        >
          <span className="text-ink-dim">{part.role} </span>
          {elementValue(part.element)}
        </span>
      ))}
    </li>
  );
}

function elementValue(element: MatchElement): string {
  return formatSi(element.value, element.kind === "inductor" ? "H" : "F", 2);
}

function describe(network: LNetwork): string {
  return `${network.shuntAt}${network.series.kind}${network.series.value}${network.shunt.kind}`;
}

function Panel({
  title,
  hint,
  children,
}: {
  title: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <section className="flex min-w-0 flex-col gap-1">
      <h4 className={LABEL} title={hint}>
        {title}
      </h4>
      {children}
    </section>
  );
}
