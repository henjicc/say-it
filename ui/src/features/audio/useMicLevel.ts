import { useEffect, useState } from "react";
import { CMD, cmd, cmdSilent } from "@/lib/tauri";

/** 电平是 RMS，说话时通常只有零点零几，放大后换算成 0～1 的条长。 */
export const micLevelFraction = (value: number) => Math.min(1, value * 1.4);

/**
 * 每 50ms 读一次麦克风电平。预览优先级最低：听写、试听等占着麦克风时申请会失败，
 * 但电平照常读共用的采集；`startError` 是最近一次申请失败的原因，成功后清空。
 */
export function useMicLevel(deviceName: string) {
  const [level, setLevel] = useState(0);
  const [startError, setStartError] = useState("");

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    let ownsMic = false;
    const tick = async () => {
      try {
        await cmd(CMD.startBackendMic, { deviceName: deviceName || undefined });
        ownsMic = true;
        if (!cancelled) setStartError("");
      } catch (error) {
        if (!cancelled) setStartError(String(error));
      }
      if (cancelled) {
        if (ownsMic) cmdSilent(CMD.releaseBackendMic);
        return;
      }
      const value = await cmd<number>(CMD.getBackendMicLevel).catch(() => 0);
      if (cancelled) return;
      setLevel(value || 0);
      timer = window.setTimeout(tick, 50);
    };
    void tick();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      if (ownsMic) cmdSilent(CMD.releaseBackendMic);
    };
  }, [deviceName]);

  return { level, startError };
}
