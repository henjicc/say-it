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
  const deviceName = useDictPrefs.getState().prefs.micDeviceId || undefined;
  const result = await cmd<{
    sampleRate?: number;
    channels?: number;
    reused?: boolean;
    deviceName?: string | null;
    fallback?: boolean;
  }>(CMD.startBackendMic, { deviceName });
  backendMicSampleRate = result.sampleRate || 48000;
  pushLog(
    `后端麦克风已${result.reused ? "复用" : "激活"}：${backendMicSampleRate}Hz / ${result.channels || 1}ch${
      result.deviceName ? ` / ${result.deviceName}` : " / 默认设备"
    }`,
  );
  if (result.fallback) {
    pushLog(`所选麦克风未找到，已回退到默认设备。`);
  }
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
