import { statusStore } from "./statusStore";
import type { TranscriberStatus } from "./types";

export const useTranscriberStore = statusStore<TranscriberStatus>((event) =>
  event.type === "TranscriberUpdate" ? event.data.status : null,
);
