import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Button, Input } from "../../components/BaseControls";
import {
  ALERT,
  BTN,
  BTN_DANGER,
  BTN_PRIMARY,
  BTN_SM,
  CHIP,
  FIELD,
  LABEL,
} from "../../components/controls";
import { Select } from "../../components/Select";
import {
  CPS_LIBRARY_KEY,
  cancelCpsJob,
  convertCpsCodeplug,
  cpsCodeplugQuery,
  cpsJobsQuery,
  cpsLibraryQuery,
  cpsPortsQuery,
  identifyRadio,
  mergeCpsCodeplug,
  radioModelsQuery,
  readRadio,
  writeRadio,
} from "../../lib/api";
import type {
  ConversionReport,
  CpsCodeplugInfo,
  CpsJob,
  CpsPort,
  CpsUser,
  MergeMode,
  RadioModelDescriptor,
} from "../../lib/types";
import { CodeplugView } from "./CodeplugView";
import {
  anyActive,
  candidateModels,
  describeJob,
  jobPercent,
  latestJob,
  modelLabel,
  portOptions,
} from "./cps";
import { LibraryPanel } from "./LibraryPanel";
import { ReportView } from "./ReportView";

const MERGE_MODES: { value: MergeMode; label: string }[] = [
  { value: "union", label: "Add what is missing" },
  { value: "append", label: "Append" },
  { value: "replace", label: "Replace" },
];

export function CpsPanel() {
  const ports = useQuery(cpsPortsQuery());
  const models = useQuery(radioModelsQuery());
  const library = useQuery(cpsLibraryQuery());
  const queryClient = useQueryClient();

  const [port, setPort] = useState("");
  const [modelId, setModelId] = useState("");
  const [userId, setUserId] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [mergeSource, setMergeSource] = useState(0);
  const [mergeMode, setMergeMode] = useState<MergeMode>("union");
  const [targetModel, setTargetModel] = useState("");
  const [report, setReport] = useState<ConversionReport | null>(null);
  const [name, setName] = useState("");

  const jobs = useQuery(cpsJobsQuery(true));
  const running = anyActive(jobs.data?.jobs ?? []);
  const job = latestJob(jobs.data?.jobs ?? []);
  const codeplug = useQuery(cpsCodeplugQuery(selected));

  const chosenPort: CpsPort | null =
    (ports.data?.ports ?? []).find((entry) => entry.port === port) ?? null;
  const offered = useMemo(
    () => candidateModels(chosenPort, models.data?.models ?? []),
    [chosenPort, models.data?.models],
  );
  const model = modelId === "" ? (offered[0]?.id ?? "") : modelId;
  const users = library.data?.users ?? [];
  const codeplugs = library.data?.codeplugs ?? [];

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: CPS_LIBRARY_KEY });
    await jobs.refetch();
  };

  const identify = useMutation({
    mutationFn: async () => identifyRadio(model, port),
  });

  const read = useMutation({
    mutationFn: async () =>
      readRadio({
        model_id: model,
        port,
        name: name.trim() === "" ? undefined : name.trim(),
        user_id: userId === 0 ? undefined : userId,
      }),
    onSuccess: refresh,
  });

  const write = useMutation({
    mutationFn: async () => {
      if (selected === null) {
        throw new Error("pick a codeplug first");
      }
      return writeRadio({
        model_id: model,
        port,
        codeplug_id: selected,
        user_id: userId === 0 ? undefined : userId,
        confirm: true,
      });
    },
    onSuccess: refresh,
  });

  const { convert, mergeIn } = useCodeplugEdits({
    selected,
    target: targetModel === "" ? model : targetModel,
    userId,
    mergeSource,
    mergeMode,
    onReport: setReport,
    refresh,
    refetchCodeplug: codeplug.refetch,
  });

  const stop = useMutation({
    mutationFn: async (id: number) => cancelCpsJob(id),
    onSuccess: refresh,
  });

  const failure =
    identify.error ?? read.error ?? write.error ?? convert.error ?? mergeIn.error ?? null;

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <section className="flex flex-wrap items-end gap-2 rounded-control border border-line bg-panel-2 p-2">
        <RadioFields
          ports={ports.data?.ports ?? []}
          port={port}
          onPort={setPort}
          offered={offered}
          model={model}
          onModel={setModelId}
          users={users}
          userId={userId}
          onUser={setUserId}
        />
        <Button
          type="button"
          className={BTN}
          disabled={port === "" || model === "" || identify.isPending || running}
          onClick={() => identify.mutate()}
        >
          Identify
        </Button>
        <Input
          className={`${FIELD} w-40`}
          aria-label="Name for the read codeplug"
          placeholder="Name the read"
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
        <Button
          type="button"
          className={BTN_PRIMARY}
          disabled={port === "" || model === "" || running}
          onClick={() => read.mutate()}
        >
          Read radio
        </Button>
        <Button
          type="button"
          className={BTN_DANGER}
          disabled={port === "" || model === "" || selected === null || running}
          onClick={() => write.mutate()}
          title="Reads the radio first, then writes back only the blocks that differ"
        >
          Write to radio
        </Button>
        {identify.data !== undefined && (
          <span className={CHIP} title="What the radio answered">
            {identify.data.reported_model}
            {identify.data.firmware === undefined ? "" : ` · ${identify.data.firmware}`}
          </span>
        )}
      </section>

      {job !== null && <JobBar job={job} onStop={() => stop.mutate(job.id)} />}

      {failure !== null && <p className={ALERT}>{failure.message}</p>}

      <div className="flex min-h-0 flex-1 gap-3">
        <aside className="flex w-72 shrink-0 flex-col">
          <LibraryPanel selected={selected} onSelect={setSelected} />
        </aside>

        <div className="flex min-h-0 flex-1 flex-col gap-2">
          <CodeplugTools
            models={models.data?.models ?? []}
            target={targetModel === "" ? model : targetModel}
            onTarget={setTargetModel}
            selected={selected}
            codeplugs={codeplugs}
            mergeSource={mergeSource}
            onMergeSource={setMergeSource}
            mergeMode={mergeMode}
            onMergeMode={setMergeMode}
            converting={convert.isPending}
            onConvert={() => convert.mutate()}
            merging={mergeIn.isPending}
            onMerge={() => mergeIn.mutate()}
          />

          <ReportView report={report ?? job?.report ?? null} />

          {codeplug.data === undefined ? (
            <p className="text-xs text-ink-dim">
              {selected === null
                ? "Pick a codeplug on the left, or read one off a radio."
                : "Loading the codeplug…"}
            </p>
          ) : (
            <CodeplugView codeplug={codeplug.data.codeplug} />
          )}
        </div>
      </div>
    </div>
  );
}

function useCodeplugEdits({
  selected,
  target,
  userId,
  mergeSource,
  mergeMode,
  onReport,
  refresh,
  refetchCodeplug,
}: {
  selected: number | null;
  target: string;
  userId: number;
  mergeSource: number;
  mergeMode: MergeMode;
  onReport: (report: ConversionReport) => void;
  refresh: () => Promise<void>;
  refetchCodeplug: () => Promise<unknown>;
}) {
  const convert = useMutation({
    mutationFn: async () => {
      if (selected === null) {
        throw new Error("pick a codeplug first");
      }
      return convertCpsCodeplug(selected, {
        target_model_id: target,
        user_id: userId === 0 ? undefined : userId,
        store: true,
      });
    },
    onSuccess: async (result) => {
      onReport(result.report);
      await refresh();
    },
  });

  const mergeIn = useMutation({
    mutationFn: async () => {
      if (selected === null || mergeSource === 0) {
        throw new Error("pick both codeplugs first");
      }
      return mergeCpsCodeplug(selected, { source_id: mergeSource, mode: mergeMode, parts: [] });
    },
    onSuccess: async (result) => {
      onReport(result.report);
      await refresh();
      await refetchCodeplug();
    },
  });

  return { convert, mergeIn };
}

function JobBar({ job, onStop }: { job: CpsJob; onStop: () => void }) {
  return (
    <section className="flex items-center gap-3 rounded-control border border-line bg-panel-2 px-2 py-1.5">
      <span className="min-w-0 flex-1 truncate font-mono text-xs text-ink">{describeJob(job)}</span>
      <div className="h-1 w-40 overflow-hidden rounded-full bg-panel-3">
        <div
          className="h-full bg-accent transition-[width] duration-200"
          style={{ width: `${jobPercent(job)}%` }}
        />
      </div>
      {(job.state === "running" || job.state === "pending") && (
        <Button type="button" className={BTN_SM} onClick={onStop}>
          Stop
        </Button>
      )}
    </section>
  );
}

interface CodeplugToolsProps {
  models: readonly RadioModelDescriptor[];
  target: string;
  onTarget: (model: string) => void;
  selected: number | null;
  codeplugs: readonly CpsCodeplugInfo[];
  mergeSource: number;
  onMergeSource: (id: number) => void;
  mergeMode: MergeMode;
  onMergeMode: (mode: MergeMode) => void;
  converting: boolean;
  onConvert: () => void;
  merging: boolean;
  onMerge: () => void;
}

function CodeplugTools({
  models,
  target,
  onTarget,
  selected,
  codeplugs,
  mergeSource,
  onMergeSource,
  mergeMode,
  onMergeMode,
  converting,
  onConvert,
  merging,
  onMerge,
}: CodeplugToolsProps) {
  return (
    <section className="flex flex-wrap items-end gap-2">
      <label className="flex flex-col gap-1">
        <span className={LABEL}>Copy to</span>
        <Select
          label="Target radio model"
          className="w-52"
          value={target}
          options={models.map((entry) => ({
            value: entry.id,
            label: modelLabel(entry),
          }))}
          onChange={onTarget}
        />
      </label>
      <Button
        type="button"
        className={BTN}
        disabled={selected === null || converting}
        onClick={onConvert}
        title="Fits this codeplug to the target radio and stores it as a new one"
      >
        Copy for that radio
      </Button>
      <label className="flex flex-col gap-1">
        <span className={LABEL}>Take entries from</span>
        <Select
          label="Merge source"
          className="w-52"
          value={mergeSource}
          options={[
            { value: 0, label: "Pick a codeplug…" },
            ...codeplugs
              .filter((info) => info.id !== selected)
              .map((info) => ({ value: info.id, label: info.name })),
          ]}
          onChange={onMergeSource}
        />
      </label>
      <Select
        label="Merge mode"
        className="w-44"
        value={mergeMode}
        options={MERGE_MODES}
        onChange={onMergeMode}
      />
      <Button
        type="button"
        className={BTN}
        disabled={selected === null || mergeSource === 0 || merging}
        onClick={onMerge}
      >
        Merge in
      </Button>
    </section>
  );
}

interface RadioFieldsProps {
  ports: readonly CpsPort[];
  port: string;
  onPort: (port: string) => void;
  offered: readonly RadioModelDescriptor[];
  model: string;
  onModel: (model: string) => void;
  users: readonly CpsUser[];
  userId: number;
  onUser: (id: number) => void;
}

function RadioFields({
  ports,
  port,
  onPort,
  offered,
  model,
  onModel,
  users,
  userId,
  onUser,
}: RadioFieldsProps) {
  return (
    <>
      <label className="flex flex-col gap-1">
        <span className={LABEL}>Port</span>
        <Select
          label="Serial port"
          className="w-64"
          value={port}
          options={[{ value: "", label: "Pick a port…" }, ...portOptions(ports)]}
          onChange={onPort}
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className={LABEL}>Radio</span>
        <Select
          label="Radio model"
          className="w-52"
          value={model}
          options={offered.map((entry) => ({ value: entry.id, label: modelLabel(entry) }))}
          onChange={onModel}
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className={LABEL}>Operator</span>
        <Select
          label="Operator"
          className="w-40"
          value={userId}
          options={[
            { value: 0, label: "Leave as read" },
            ...users.map((user) => ({
              value: user.id,
              label: user.callsign ?? user.name,
            })),
          ]}
          onChange={onUser}
        />
      </label>
    </>
  );
}
