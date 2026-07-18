import { create } from "zustand";

interface PluginImportState {
  pendingPaths: string[];
  enqueue: (paths: string[]) => void;
  finishCurrent: () => void;
}

export const usePluginImportStore = create<PluginImportState>((set) => ({
  pendingPaths: [],
  enqueue: (paths) => set((state) => ({
    pendingPaths: [...state.pendingPaths, ...paths.filter((path) => !state.pendingPaths.includes(path))],
  })),
  finishCurrent: () => set((state) => ({ pendingPaths: state.pendingPaths.slice(1) })),
}));
