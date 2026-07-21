import { create } from "zustand";

type Tone = "" | "ok" | "err";

export interface Meters {
  olufs: string;
  orms: string;
  opeak: string;
  plufs: string;
  prms: string;
  ppeak: string;
  clip: string;
}

const emptyMeters: Meters = {
  olufs: "-",
  orms: "-",
  opeak: "-",
  plufs: "-",
  prms: "-",
  ppeak: "-",
  clip: "-",
};

interface AudioState {
  recording: boolean;
  recInfo: string;
  recTone: Tone;
  canPlay: boolean;
  meters: Meters;
  labStatus: string;
  labStatusTone: Tone;
}

export const useAudioStore = create<AudioState>(() => ({
  recording: false,
  recInfo: "",
  recTone: "",
  canPlay: false,
  meters: { ...emptyMeters },
  labStatus: "参数改动实时生效并自动保存到速记。",
  labStatusTone: "",
}));

export { emptyMeters };
