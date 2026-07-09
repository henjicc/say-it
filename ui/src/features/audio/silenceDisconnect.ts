import { base64ToFloat32, measure } from "@/lib/audio-dsp";
import { useDictPrefs, type DictPrefs } from "@/store/useDictPrefs";

export function rawChunkRms(base64: string) {
  const samples = base64ToFloat32(base64);
  if (!samples.length) return 0;
  return measure(samples).rms;
}

export function silenceDisconnectPrefs(): Pick<
  DictPrefs,
  | "dictationSilenceDisconnectEnabled"
  | "dictationSilenceDisconnectMs"
  | "dictationSilenceThreshold"
  | "subtitleSilenceDisconnectEnabled"
  | "subtitleSilenceDisconnectMs"
  | "subtitleSilenceThreshold"
> {
  const prefs = useDictPrefs.getState().prefs;
  return {
    dictationSilenceDisconnectEnabled: prefs.dictationSilenceDisconnectEnabled !== false,
    dictationSilenceDisconnectMs: Math.max(1000, Number(prefs.dictationSilenceDisconnectMs) || 5000),
    dictationSilenceThreshold: Math.min(0.1, Math.max(0.0001, Number(prefs.dictationSilenceThreshold) || 0.0001)),
    subtitleSilenceDisconnectEnabled: prefs.subtitleSilenceDisconnectEnabled !== false,
    subtitleSilenceDisconnectMs: Math.max(1000, Number(prefs.subtitleSilenceDisconnectMs) || 5000),
    subtitleSilenceThreshold: Math.min(0.1, Math.max(0.0001, Number(prefs.subtitleSilenceThreshold) || 0.0001)),
  };
}
