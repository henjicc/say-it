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

export type PlaybackTrack = "original" | "processed";

interface AudioState {
  recording: boolean;
  recInfo: string;
  recTone: Tone;
  canPlay: boolean;
  meters: Meters;
  labStatus: string;
  labStatusTone: Tone;
  /** 正在试听的音轨；暂停时保留，播完或停止后为 null。 */
  playback: { track: PlaybackTrack; paused: boolean } | null;
}

export const useAudioStore = create<AudioState>(() => ({
  recording: false,
  recInfo: "",
  recTone: "",
  canPlay: false,
  meters: { ...emptyMeters },
  labStatus: "参数改动实时生效并自动保存到速记。",
  labStatusTone: "",
  playback: null,
}));

export { emptyMeters };
