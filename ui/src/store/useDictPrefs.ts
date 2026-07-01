import { create } from "zustand";
import { dspDefaults, dspParamsFromPrefs, type DspParams } from "@/lib/audio-dsp";
import { CMD, cmdSilent } from "@/lib/tauri";
import {
  defaultLocalRules,
  mergeLocalRules,
  type LocalRule,
} from "@/features/dictation/localRulesEngine";

export type CueKind = "none" | "beep-up" | "beep-down" | "beep-double" | "custom";

export interface DictPrefs extends DspParams {
  keepAliveMs: number;
  cueEnabled: boolean;
  cueStart: CueKind;
  cueEnd: CueKind;
  debugLog: boolean;
  localRulesEnabled: boolean;
  localRules: LocalRule[];
}

const DICT_PREFS_KEY = "sayItDictPrefs";

function defaults(): DictPrefs {
  return {
    keepAliveMs: 60000,
    cueEnabled: true,
    cueStart: "beep-up",
    cueEnd: "beep-down",
    debugLog: false,
    localRulesEnabled: false,
    localRules: defaultLocalRules(),
    ...dspDefaults,
  };
}

function readStored(): DictPrefs {
  const base = defaults();
  try {
    const raw = localStorage.getItem(DICT_PREFS_KEY);
    if (raw) Object.assign(base, JSON.parse(raw));
  } catch {
    /* noop */
  }
  base.localRules = mergeLocalRules(base.localRules);
  return base;
}

function persist(prefs: DictPrefs) {
  try {
    localStorage.setItem(DICT_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* noop */
  }
}

interface DictPrefsState {
  prefs: DictPrefs;
  patch: (partial: Partial<DictPrefs>) => void;
  resetLocalRules: () => void;
  dspParams: () => DspParams;
}

export const useDictPrefs = create<DictPrefsState>((set, get) => ({
  prefs: readStored(),
  patch: (partial) => {
    const next = { ...get().prefs, ...partial };
    persist(next);
    set({ prefs: next });
    if ("debugLog" in partial) {
      cmdSilent(CMD.setDebugLog, { enabled: !!next.debugLog });
    }
  },
  resetLocalRules: () => get().patch({ localRules: defaultLocalRules() }),
  dspParams: () => dspParamsFromPrefs(get().prefs),
}));

export function syncDebugLogToBackend() {
  cmdSilent(CMD.setDebugLog, { enabled: !!useDictPrefs.getState().prefs.debugLog });
}
