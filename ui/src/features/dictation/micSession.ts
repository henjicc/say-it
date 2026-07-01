import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";

let micShutdownTimer: ReturnType<typeof setTimeout> | null = null;
let backendMicSampleRate = 48000;

export function getBackendMicSampleRate() {
  return backendMicSampleRate;
}

export function clearMicShutdownTimer() {
  if (micShutdownTimer) {
    clearTimeout(micShutdownTimer);
    micShutdownTimer = null;
  }
}

export async function ensureMic(pushLog: (message: string) => void) {
  const result = await cmd<{ sampleRate?: number; channels?: number; reused?: boolean }>(
    CMD.startBackendMic,
  );
  backendMicSampleRate = result.sampleRate || 48000;
  pushLog(
    `后端麦克风已${result.reused ? "复用" : "激活"}：${backendMicSampleRate}Hz / ${result.channels || 1}ch`,
  );
}

export async function shutdownMic() {
  clearMicShutdownTimer();
  await cmdSilent(CMD.releaseBackendMic);
}

export function scheduleMicShutdown(pushLog: (message: string) => void) {
  clearMicShutdownTimer();
  const ms = useDictPrefs.getState().prefs.keepAliveMs | 0;
  if (ms <= 0) {
    shutdownMic();
    return;
  }
  micShutdownTimer = setTimeout(() => {
    pushLog("麦克风保活到期，释放设备。");
    shutdownMic();
  }, ms);
}
