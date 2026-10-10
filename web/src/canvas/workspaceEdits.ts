import type { WorkspaceDetail, WorkspaceSnapshot } from "../lib/types";

export type Edit = (snapshot: WorkspaceSnapshot) => WorkspaceSnapshot;

interface Flight {
  edits: readonly Edit[];
  base: WorkspaceDetail;
}

interface Lane {
  pending: Edit[];
  flight: Flight | null;
}

export interface Outgoing {
  revision: number;
  snapshot: WorkspaceSnapshot;
}

interface Viewed {
  detail: WorkspaceDetail;
  version: number;
  shown: WorkspaceDetail;
}

export class WorkspaceEdits {
  readonly #lanes = new Map<number, Lane>();
  readonly #fit: (snapshot: WorkspaceSnapshot) => WorkspaceSnapshot;
  #version = 0;
  #viewed: Viewed | null = null;

  constructor(fit: (snapshot: WorkspaceSnapshot) => WorkspaceSnapshot = (snapshot) => snapshot) {
    this.#fit = fit;
  }

  push(id: number, edit: Edit): void {
    this.#lane(id).pending.push(edit);
    this.#version += 1;
  }

  view(detail: WorkspaceDetail): WorkspaceDetail {
    const viewed = this.#viewed;
    if (viewed !== null && viewed.detail === detail && viewed.version === this.#version) {
      return viewed.shown;
    }
    const shown = this.#show(detail);
    this.#viewed = { detail, version: this.#version, shown };
    return shown;
  }

  #show(detail: WorkspaceDetail): WorkspaceDetail {
    const lane = this.#lanes.get(detail.id);
    if (lane === undefined || (lane.flight === null && lane.pending.length === 0)) {
      return detail;
    }
    const base = lane.flight?.base ?? detail;
    const edits = [...(lane.flight?.edits ?? []), ...lane.pending];
    return { ...base, snapshot: this.#replay(base.snapshot, edits) };
  }

  take(id: number, base: WorkspaceDetail): Outgoing | null {
    const lane = this.#lanes.get(id);
    if (lane === undefined || lane.flight !== null || lane.pending.length === 0) {
      return null;
    }
    const edits = lane.pending;
    lane.pending = [];
    lane.flight = { edits, base };
    this.#version += 1;
    return { revision: base.revision, snapshot: this.#replay(base.snapshot, edits) };
  }

  landed(id: number): void {
    const lane = this.#lanes.get(id);
    if (lane === undefined) {
      return;
    }
    lane.flight = null;
    this.#version += 1;
    if (lane.pending.length === 0) {
      this.#lanes.delete(id);
    }
  }

  waiting(id: number): boolean {
    return (this.#lanes.get(id)?.pending.length ?? 0) > 0;
  }

  #lane(id: number): Lane {
    let lane = this.#lanes.get(id);
    if (lane === undefined) {
      lane = { pending: [], flight: null };
      this.#lanes.set(id, lane);
    }
    return lane;
  }

  #replay(snapshot: WorkspaceSnapshot, edits: readonly Edit[]): WorkspaceSnapshot {
    return this.#fit(edits.reduce((held, edit) => edit(held), snapshot));
  }
}

export function newer(
  cached: WorkspaceDetail | undefined,
  stored: WorkspaceDetail,
): WorkspaceDetail {
  return cached === undefined || cached.revision <= stored.revision ? stored : cached;
}
