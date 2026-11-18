import { create } from "zustand";
import type { NodeKind, NodeTypeInfo, PatchCatalog, ProcessorType } from "./types";

interface CatalogStore {
  catalog: PatchCatalog | null;
  types: ReadonlyMap<NodeKind, NodeTypeInfo>;
}

export const useCatalogStore = create<CatalogStore>(() => ({ catalog: null, types: new Map() }));

export function loadCatalog(catalog: PatchCatalog): void {
  if (useCatalogStore.getState().catalog === catalog) {
    return;
  }
  useCatalogStore.setState({
    catalog,
    types: new Map(catalog.nodes.map((type) => [type.kind, type])),
  });
}

export function catalogType(kind: NodeKind): NodeTypeInfo | undefined {
  return useCatalogStore.getState().types.get(kind);
}

/** The processor type a processor node runs, as the loaded catalog lists it. */
export function catalogProcessor(typeId: string): ProcessorType | undefined {
  return useCatalogStore.getState().catalog?.processors?.find((type) => type.type_id === typeId);
}
