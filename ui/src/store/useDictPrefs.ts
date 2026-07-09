import { create } from "zustand";
import { dspDefaults, dspParamsFromPrefs, type DspParams } from "@/lib/audio-dsp";
import { CMD, cmdSilent } from "@/lib/tauri";
import {
  DEFAULT_REALTIME_ASR_MODEL,
  isSupportedDictationModel,
} from "@/features/asr/modelOptions";
import {
  defaultLocalRules,
  mergeLocalRules,
  type LocalRule,
} from "@/features/dictation/localRulesEngine";

export type CueKind = "none" | "beep-up" | "beep-down" | "beep-double" | "custom";

export interface DictPrefs extends DspParams {
  /** 语音输入使用的识别模型：实时模型边说边出字，非实时模型停止后再识别。 */
  asrModel: string;
  keepAliveMs: number;
  cueEnabled: boolean;
  cueStart: CueKind;
  cueEnd: CueKind;
  debugLog: boolean;
  localRulesEnabled: boolean;
  localRules: LocalRule[];
  /** 指定麦克风设备名；空字符串表示使用系统默认输入设备。语音输入和实时字幕的"麦克风"来源共用这一设置。 */
  micDeviceId: string;
  dictationSilenceDisconnectEnabled: boolean;
  dictationSilenceDisconnectMs: number;
  dictationSilenceThreshold: number;
  subtitleSilenceDisconnectEnabled: boolean;
  subtitleSilenceDisconnectMs: number;
  subtitleSilenceThreshold: number;
}

const DICT_PREFS_KEY = "sayItDictPrefs";

function defaults(): DictPrefs {
  return {
    asrModel: DEFAULT_REALTIME_ASR_MODEL,
    keepAliveMs: 60000,
    cueEnabled: true,
    cueStart: "beep-up",
    cueEnd: "beep-down",
    debugLog: false,
    localRulesEnabled: false,
    localRules: defaultLocalRules(),
    micDeviceId: "",
    dictationSilenceDisconnectEnabled: true,
    dictationSilenceDisconnectMs: 5000,
    dictationSilenceThreshold: 0.0001,
    subtitleSilenceDisconnectEnabled: true,
    subtitleSilenceDisconnectMs: 5000,
    subtitleSilenceThreshold: 0.0001,
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
  const legacy = base as DictPrefs & {
    silenceDisconnectEnabled?: boolean;
    silenceThreshold?: number;
  };
  if (typeof legacy.silenceDisconnectEnabled === "boolean") {
    base.dictationSilenceDisconnectEnabled = legacy.silenceDisconnectEnabled;
    base.subtitleSilenceDisconnectEnabled = legacy.silenceDisconnectEnabled;
  }
  if (typeof legacy.silenceThreshold === "number") {
    base.dictationSilenceThreshold = legacy.silenceThreshold;
  }
  base.dictationSilenceThreshold = Math.min(0.1, Math.max(0.0001, Number(base.dictationSilenceThreshold) || 0.0001));
  base.subtitleSilenceThreshold = Math.min(0.1, Math.max(0.0001, Number(base.subtitleSilenceThreshold) || 0.0001));
  if (!isSupportedDictationModel(base.asrModel)) {
    base.asrModel = DEFAULT_REALTIME_ASR_MODEL;
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
