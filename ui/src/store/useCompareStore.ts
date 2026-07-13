import { create } from "zustand";
import { CMD, cmd } from "@/lib/tauri";
import type { SelectedTranscriptionFile } from "@/store/useTranscriptionStore";

export type CompareSourceMode = "record" | "upload";
export type ComparePhase = "idle" | "recording" | "playing" | "finalizing";
export type CompareCellStatus =
  | "idle"
  | "queued"
  | "connecting"
  | "streaming"
  | "uploading"
  | "recognizing"
  | "done"
  | "error";

export const COMPARE_COLS = 2;
export const COMPARE_MIN_ROWS = 1;
export const COMPARE_MAX_ROWS = 4;

export interface ComparePrefs {
  sourceMode: CompareSourceMode;
  cellModels: string[];
}

export interface CompareCellRuntime {
  status: CompareCellStatus;
  text: string;
  errorMessage: string;
}

interface CompareState {
  prefs: ComparePrefs;
  selectedFile: SelectedTranscriptionFile | null;
  phase: ComparePhase;
  globalError: string;
  playbackProgress: { currentMs: number; durationMs: number } | null;
  cellRuntime: CompareCellRuntime[];

  patch: (partial: Partial<ComparePrefs>) => void;
  setCellModel: (index: number, value: string) => void;
  addRow: () => void;
  removeRow: () => void;
  setSelectedFile: (file: SelectedTranscriptionFile | null) => void;
  setRuntime: (
    partial: Partial<
      Pick<CompareState, "phase" | "globalError" | "playbackProgress">
    >,
  ) => void;
  patchCellRuntime: (index: number, partial: Partial<CompareCellRuntime>) => void;
  resetRuntime: () => void;
}

const COMPARE_PREFS_KEY = "sayItComparePrefs";

const emptyCellRuntime = (): CompareCellRuntime => ({ status: "idle", text: "", errorMessage: "" });

function defaults(): ComparePrefs {
  return { sourceMode: "record", cellModels: ["", ""] };
}

function clampPrefs(prefs: ComparePrefs): ComparePrefs {
  const cellModels = Array.isArray(prefs.cellModels) ? prefs.cellModels.map((v) => String(v || "")) : [];
  const rows = Math.min(
    COMPARE_MAX_ROWS,
    Math.max(COMPARE_MIN_ROWS, Math.round(cellModels.length / COMPARE_COLS) || COMPARE_MIN_ROWS),
  );
  const normalized = Array.from({ length: rows * COMPARE_COLS }, (_, i) => cellModels[i] || "");
  return {
    sourceMode: prefs.sourceMode === "upload" ? "upload" : "record",
    cellModels: normalized,
  };
}

function readStored(): ComparePrefs {
  const base = defaults();
  try {
    const raw = localStorage.getItem(COMPARE_PREFS_KEY);
    if (raw) Object.assign(base, JSON.parse(raw));
  } catch {
    /* noop */
  }
  return clampPrefs(base);
}

function persist(prefs: ComparePrefs) {
  try {
    localStorage.setItem(COMPARE_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* noop */
  }
}

export const useCompareStore = create<CompareState>((set, get) => ({
  prefs: readStored(),
  selectedFile: null,
  phase: "idle",
  globalError: "",
  playbackProgress: null,
  cellRuntime: readStored().cellModels.map(emptyCellRuntime),

  patch: (partial) => {
    const next = clampPrefs({ ...get().prefs, ...partial });
    void save(next, () => set({ prefs: next, cellRuntime: next.cellModels.map(emptyCellRuntime) }));
  },
  setCellModel: (index, value) => {
    const cellModels = get().prefs.cellModels.slice();
    if (index < 0 || index >= cellModels.length) return;
    cellModels[index] = value;
    const next = clampPrefs({ ...get().prefs, cellModels });
    void save(next, () => set({ prefs: next }));
  },
  addRow: () => {
    const { cellModels } = get().prefs;
    if (cellModels.length / COMPARE_COLS >= COMPARE_MAX_ROWS) return;
    const next = clampPrefs({ ...get().prefs, cellModels: [...cellModels, ...Array(COMPARE_COLS).fill("")] });
    void save(next, () => set({ prefs: next, cellRuntime: next.cellModels.map(emptyCellRuntime) }));
  },
  removeRow: () => {
    const { cellModels } = get().prefs;
    if (cellModels.length / COMPARE_COLS <= COMPARE_MIN_ROWS) return;
    const next = clampPrefs({ ...get().prefs, cellModels: cellModels.slice(0, cellModels.length - COMPARE_COLS) });
    void save(next, () => set({ prefs: next, cellRuntime: next.cellModels.map(emptyCellRuntime) }));
  },
  setSelectedFile: (file) => set({ selectedFile: file }),
  setRuntime: (partial) => set(partial),
  patchCellRuntime: (index, partial) =>
    set((state) => {
      if (index < 0 || index >= state.cellRuntime.length) return state;
      const cellRuntime = state.cellRuntime.slice();
      cellRuntime[index] = { ...cellRuntime[index], ...partial };
      return { cellRuntime };
    }),
  resetRuntime: () =>
    set((state) => ({
      phase: "idle",
      globalError: "",
      playbackProgress: null,
      cellRuntime: state.prefs.cellModels.map(emptyCellRuntime),
    })),
}));

async function save(next: ComparePrefs, commit: () => void) { await cmd(CMD.updateAppSettings, { domain: "comparison", value: next }); persist(next); commit(); }
export function hydrateComparePrefs(value: Record<string, unknown>) { const next = clampPrefs({ ...readStored(), ...value } as ComparePrefs); persist(next); useCompareStore.setState({ prefs: next, cellRuntime: next.cellModels.map(emptyCellRuntime) }); }
