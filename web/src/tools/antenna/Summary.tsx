import { LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Icon } from "../../components/Icon";
import type { AntennaPolarization, AntennaSimulation } from "../../lib/types";
import { formatImpedance } from "../nanovna/nanovna";
import { formatSwr, matchQuality } from "../nanovna/Readouts";
import { type Matching, roundedHz, type Span } from "./performance";

const DIPOLE_DBI = 2.15;

const POLARIZATION: Record<AntennaPolarization, string> = {
  horizontal: "Horizontal",
  vertical: "Vertical",
  mixed: "Mixed",
};

export function Summary({
  simulation,
  frequencyHz,
  grounded,
  matching,
  swr,
  span,
}: {
  simulation: AntennaSimulation;
  frequencyHz: number;
  grounded: boolean;
  matching: Matching | null;
  swr: number;
  span: Span | null;
}) {
  const quality = matchQuality(swr);
  return (
    <section
      aria-label="Simulated performance"
      className="flex flex-wrap items-end gap-x-8 gap-y-3 rounded-[3px] border border-line bg-panel-2/60 px-4 py-3"
    >
      <Big label={`Feedpoint at ${formatHz(frequencyHz)}`}>
        {formatImpedance(simulation.impedance)}
      </Big>
      <div className="flex flex-col gap-1">
        <span className={LABEL} title={matching?.detail}>
          SWR{matching === null || matching.id === "direct" ? "" : `, ${matching.label}`}
        </span>
        <span className="flex items-baseline gap-3">
          <span className="font-mono text-4xl leading-none font-semibold text-ink tabular-nums">
            {formatSwr(swr)}
          </span>
          <span className={`inline-flex items-center gap-1 text-xs font-medium ${quality.tone}`}>
            <Icon glyph={quality.glyph} size={12} />
            {quality.label}
          </span>
        </span>
      </div>
      <Big label={grounded ? "Gain over ground" : "Gain"}>{simulation.gain_dbi.toFixed(2)} dBi</Big>
      <Small label="Over a dipole" value={`${(simulation.gain_dbi - DIPOLE_DBI).toFixed(2)} dBd`} />
      <Small label="Front to back" value={`${simulation.front_to_back_db.toFixed(1)} dB`} />
      <Small
        label={grounded ? "Takeoff" : "Elevation"}
        value={`${simulation.takeoff_deg.toFixed(0)}°`}
      />
      <Small
        label="Beamwidth"
        value={`${angle(simulation.beamwidth_azimuth_deg)} / ${angle(simulation.beamwidth_elevation_deg)}`}
        title="Horizontal and vertical width at −3 dB"
      />
      <Small label="Efficiency" value={`${(simulation.efficiency * 100).toFixed(1)} %`} />
      <Small label="Polarisation" value={POLARIZATION[simulation.polarization]} />
      <Small label="Under 2:1" value={spanText(span)} title="Bandwidth with SWR under 2" />
    </section>
  );
}

function angle(value: number | null | undefined): string {
  return value == null ? "omni" : `${value.toFixed(0)}°`;
}

function spanText(span: Span | null): string {
  if (span === null) {
    return "-";
  }
  const width = span.highHz - span.lowHz;
  const open = span.openLow || span.openHigh ? "> " : "";
  return `${open}${formatHz(roundedHz(width))}`;
}

function Big({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className={LABEL}>{label}</span>
      <span className="font-mono text-xl leading-none text-ink tabular-nums">{children}</span>
    </div>
  );
}

function Small({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div className="flex flex-col gap-1" title={title}>
      <span className={LABEL}>{label}</span>
      <span className="font-mono text-sm leading-none text-ink-dim tabular-nums">{value}</span>
    </div>
  );
}
