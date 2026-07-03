import { create } from "zustand";

export type TranscriptionTab = "transcribe" | "align";

export interface SelectedTranscriptionFile {
  path: string;
  name: string;
  size: number;
}

interface TranscriptionState {
  tab: TranscriptionTab;
  selectedFile: SelectedTranscriptionFile | null;
  setTab: (tab: TranscriptionTab) => void;
  setSelectedFile: (file: SelectedTranscriptionFile | null) => void;
}

export const useTranscriptionStore = create<TranscriptionState>((set) => ({
  tab: "transcribe",
  selectedFile: null,
  setTab: (tab) => set({ tab }),
  setSelectedFile: (selectedFile) => set({ selectedFile }),
}));
