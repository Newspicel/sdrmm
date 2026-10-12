import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, EyeOff, FileUp, Plus, Save, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { Button, Form, Input } from "../../components/BaseControls";
import { ConfirmButton } from "../../components/ConfirmButton";
import { ALERT, BTN, BTN_PRIMARY, FIELD, ICON_BTN_SM, LABEL } from "../../components/controls";
import { formatHz } from "../../components/format";
import { Icon } from "../../components/Icon";
import {
  createNanoVnaAntenna,
  deleteNanoVnaAntenna,
  deleteNanoVnaMeasurement,
  NANOVNA_LIBRARY_KEY,
  nanoVnaLibraryQuery,
  nanoVnaMeasurementQuery,
  saveNanoVnaMeasurement,
} from "../../lib/api";
import type { NanoVnaAntenna, NanoVnaMeasurementInfo, NanoVnaPoint } from "../../lib/types";
import { formatSwr } from "./Readouts";
import { MAX_REFERENCES, type Reference, type ReferenceSource, storedKey } from "./references";
import { parseTouchstone, touchstonePorts } from "./touchstone";

export interface Recording {
  points: readonly NanoVnaPoint[];
  instrument: string | null;
  label: string;
}

interface Imported extends Recording {
  key: string;
}

export function AntennaLibrary({
  current,
  references,
  onToggle,
  onOpen,
  onForget,
}: {
  current: Recording | null;
  references: readonly Reference[];
  onToggle: (source: ReferenceSource) => void;
  onOpen: (source: ReferenceSource) => void;
  onForget: (key: string) => void;
}) {
  const library = useQuery(nanoVnaLibraryQuery());
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<number | null>(null);
  const [imported, setImported] = useState<Imported[]>([]);
  const refresh = () => queryClient.invalidateQueries({ queryKey: NANOVNA_LIBRARY_KEY });
  const antennas = library.data?.antennas ?? [];
  const target = antennas.find((antenna) => antenna.id === selected) ?? antennas[0] ?? null;

  const save = useMutation({
    mutationFn: async (recording: Recording) => {
      if (target === null) {
        throw new Error("Add an antenna first");
      }
      return saveNanoVnaMeasurement(target.id, {
        label: recording.label,
        note: "",
        instrument: recording.instrument ?? undefined,
        points: [...recording.points],
      });
    },
    onSuccess: refresh,
  });

  async function source(info: NanoVnaMeasurementInfo): Promise<ReferenceSource> {
    const measurement = await queryClient.fetchQuery(nanoVnaMeasurementQuery(info.id));
    return { key: storedKey(info.id), label: info.label, points: measurement.points };
  }

  const shown = (key: string) => references.find((reference) => reference.key === key) ?? null;
  const full = references.length >= MAX_REFERENCES;

  return (
    <aside className="flex min-h-0 flex-col gap-3 overflow-y-auto border-l border-line pl-4">
      <NewAntenna onCreated={(antenna) => setSelected(antenna.id)} refresh={refresh} />
      {library.isError && <p className={ALERT}>{library.error.message}</p>}
      {antennas.length === 0 && library.isSuccess && (
        <p className="text-xs text-ink-faint">Name an antenna to keep its sweeps.</p>
      )}
      <SaveBar
        target={target}
        current={current}
        pending={save.isPending}
        onSave={(label) => current !== null && save.mutate({ ...current, label })}
      />
      {save.isError && <p className={ALERT}>{save.error.message}</p>}
      <ul className="flex flex-col gap-2">
        {antennas.map((antenna) => (
          <AntennaGroup
            key={antenna.id}
            antenna={antenna}
            selected={antenna.id === target?.id}
            measurements={(library.data?.measurements ?? []).filter(
              (measurement) => measurement.antenna_id === antenna.id,
            )}
            shown={shown}
            full={full}
            onSelect={() => setSelected(antenna.id)}
            onToggle={async (info) => onToggle(await source(info))}
            onOpen={async (info) => onOpen(await source(info))}
            onDeleted={(keys) => {
              keys.forEach(onForget);
              void refresh();
            }}
          />
        ))}
      </ul>
      <Files
        files={imported}
        shown={shown}
        full={full}
        canSave={target !== null}
        onAdd={(file) => setImported((files) => [...files, file])}
        onToggle={onToggle}
        onOpen={onOpen}
        onSave={(file) => save.mutate(file)}
      />
    </aside>
  );
}

function NewAntenna({
  onCreated,
  refresh,
}: {
  onCreated: (antenna: NanoVnaAntenna) => void;
  refresh: () => Promise<void>;
}) {
  const [name, setName] = useState("");
  const create = useMutation({
    mutationFn: async () => createNanoVnaAntenna({ name: name.trim(), notes: "" }),
    onSuccess: async (antenna) => {
      setName("");
      onCreated(antenna);
      await refresh();
    },
  });
  return (
    <Form
      className="flex flex-col gap-1"
      onSubmit={(event) => {
        event.preventDefault();
        if (name.trim() !== "") {
          create.mutate();
        }
      }}
    >
      <span className={LABEL}>Antennas</span>
      <div className="flex gap-1.5">
        <Input
          aria-label="New antenna name"
          data-hotkeys="off"
          value={name}
          placeholder="New antenna"
          onChange={(event) => setName(event.target.value)}
          className={`${FIELD} flex-1`}
        />
        <Button type="submit" className={BTN} disabled={name.trim() === "" || create.isPending}>
          <Icon glyph={Plus} size={12} />
          Add
        </Button>
      </div>
      {create.isError && <p className={ALERT}>{create.error.message}</p>}
    </Form>
  );
}

function SaveBar({
  target,
  current,
  pending,
  onSave,
}: {
  target: NanoVnaAntenna | null;
  current: Recording | null;
  pending: boolean;
  onSave: (label: string) => void;
}) {
  const [label, setLabel] = useState("");
  if (target === null || current === null) {
    return null;
  }
  return (
    <Form
      className="flex gap-1.5"
      onSubmit={(event) => {
        event.preventDefault();
        onSave(label.trim() === "" ? current.label : label.trim());
        setLabel("");
      }}
    >
      <Input
        aria-label="Measurement name"
        data-hotkeys="off"
        value={label}
        placeholder={current.label}
        onChange={(event) => setLabel(event.target.value)}
        className={`${FIELD} min-w-0 flex-1`}
      />
      <Button
        type="submit"
        className={BTN_PRIMARY}
        disabled={pending}
        title={`Save this sweep to ${target.name}`}
      >
        <Icon glyph={Save} size={12} />
        Save
      </Button>
    </Form>
  );
}

function AntennaGroup({
  antenna,
  selected,
  measurements,
  shown,
  full,
  onSelect,
  onToggle,
  onOpen,
  onDeleted,
}: {
  antenna: NanoVnaAntenna;
  selected: boolean;
  measurements: NanoVnaMeasurementInfo[];
  shown: (key: string) => Reference | null;
  full: boolean;
  onSelect: () => void;
  onToggle: (info: NanoVnaMeasurementInfo) => Promise<void>;
  onOpen: (info: NanoVnaMeasurementInfo) => Promise<void>;
  onDeleted: (keys: string[]) => void;
}) {
  const removeAntenna = useMutation({
    mutationFn: async () => deleteNanoVnaAntenna(antenna.id),
    onSuccess: () => onDeleted(measurements.map((measurement) => storedKey(measurement.id))),
  });
  const removeMeasurement = useMutation({
    mutationFn: async (id: number) => deleteNanoVnaMeasurement(id),
    onSuccess: (_, id) => onDeleted([storedKey(id)]),
  });
  return (
    <li
      className={`rounded-control border ${selected ? "border-accent/60 bg-accent/5" : "border-line"}`}
    >
      <div className="flex items-center gap-1 px-2 py-1.5">
        <Button
          type="button"
          onClick={onSelect}
          className="min-w-0 flex-1 truncate text-left text-xs font-medium text-ink"
          title={selected ? "New sweeps are saved here" : "Save new sweeps here"}
        >
          {antenna.name}
        </Button>
        <span className="font-mono text-2xs text-ink-faint">{measurements.length}</span>
        <ConfirmButton
          label={`Delete ${antenna.name}`}
          size="icon"
          icon={Trash2}
          onConfirm={() => removeAntenna.mutate()}
        />
      </div>
      {measurements.length > 0 && (
        <ul className="border-t border-line/60">
          {measurements.map((measurement) => (
            <EntryRow
              key={measurement.id}
              label={measurement.label}
              detail={describe(measurement)}
              reference={shown(storedKey(measurement.id))}
              full={full}
              onToggle={() => void onToggle(measurement)}
              onOpen={() => void onOpen(measurement)}
              onDelete={() => removeMeasurement.mutate(measurement.id)}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function Files({
  files,
  shown,
  full,
  canSave,
  onAdd,
  onToggle,
  onOpen,
  onSave,
}: {
  files: Imported[];
  shown: (key: string) => Reference | null;
  full: boolean;
  canSave: boolean;
  onAdd: (file: Imported) => void;
  onToggle: (source: ReferenceSource) => void;
  onOpen: (source: ReferenceSource) => void;
  onSave: (file: Imported) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);

  async function load(file: File) {
    const ports = touchstonePorts(file.name);
    if (ports === null) {
      setError(`${file.name} is not a .s1p or .s2p file`);
      return;
    }
    const parsed = parseTouchstone(await file.text(), ports);
    if (!parsed.ok) {
      setError(`${file.name}: ${parsed.error}`);
      return;
    }
    setError(null);
    const imported = {
      key: `file:${file.name}:${Date.now()}`,
      label: file.name,
      instrument: file.name,
      points: parsed.value.points,
    };
    onAdd(imported);
    onToggle(imported);
  }

  return (
    <section className="flex flex-col gap-1">
      <Input
        ref={input}
        type="file"
        accept=".s1p,.s2p"
        className="hidden"
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (file !== undefined) {
            void load(file);
          }
        }}
      />
      <Button
        type="button"
        className={`${BTN} w-full justify-center`}
        onClick={() => input.current?.click()}
      >
        <Icon glyph={FileUp} size={12} />
        Open .s1p or .s2p
      </Button>
      {error !== null && <p className={ALERT}>{error}</p>}
      {files.length > 0 && (
        <ul className="rounded-control border border-line">
          {files.map((file) => (
            <EntryRow
              key={file.key}
              label={file.label}
              detail={`${file.points.length} points`}
              reference={shown(file.key)}
              full={full}
              onToggle={() => onToggle(file)}
              onOpen={() => onOpen(file)}
              onSave={canSave ? () => onSave(file) : undefined}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function EntryRow({
  label,
  detail,
  reference,
  full,
  onToggle,
  onOpen,
  onDelete,
  onSave,
}: {
  label: string;
  detail: string;
  reference: Reference | null;
  full: boolean;
  onToggle: () => void;
  onOpen: () => void;
  onDelete?: () => void;
  onSave?: () => void;
}) {
  return (
    <li className="group flex items-center gap-1 border-t border-line/60 px-2 py-1 first:border-t-0">
      <span
        aria-hidden
        className="h-0.5 w-3 shrink-0 rounded-full"
        style={{ background: reference?.color ?? "transparent" }}
      />
      <Button
        type="button"
        onClick={onOpen}
        title="Show as the main trace"
        className="flex min-w-0 flex-1 flex-col text-left"
      >
        <span className="truncate text-xs text-ink">{label}</span>
        <span className="truncate font-mono text-2xs text-ink-faint">{detail}</span>
      </Button>
      <Button
        type="button"
        className={ICON_BTN_SM}
        aria-label={reference === null ? `Overlay ${label}` : `Hide ${label}`}
        title={reference === null ? (full ? "Four overlays at most" : "Overlay") : "Hide overlay"}
        disabled={reference === null && full}
        onClick={onToggle}
      >
        <Icon glyph={reference === null ? Eye : EyeOff} size={12} />
      </Button>
      {onSave !== undefined && (
        <Button
          type="button"
          className={ICON_BTN_SM}
          aria-label={`Save ${label}`}
          title="Save to the antenna"
          onClick={onSave}
        >
          <Icon glyph={Save} size={12} />
        </Button>
      )}
      {onDelete !== undefined && (
        <ConfirmButton label={`Delete ${label}`} size="icon" icon={Trash2} onConfirm={onDelete} />
      )}
    </li>
  );
}

function describe(measurement: NanoVnaMeasurementInfo): string {
  const date = new Date(measurement.created_at).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
  const best =
    measurement.best_vswr == null || measurement.best_hz == null
      ? ""
      : ` · ${formatSwr(measurement.best_vswr)} at ${formatHz(measurement.best_hz)}`;
  return `${date}${best}`;
}
