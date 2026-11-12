import { useEffect, useState } from "react";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd, type SetupCheckResult, type SetupStatus } from "@/lib/tauri";

/** 引导里只给入口、不能就地授予的权限：缺失时打开系统设置对应页面。只在用到时才有检查结果。 */
export const PRIVACY_ROWS = [
  {
    id: "screenRecording",
    kind: "screen-recording",
    label: "读取软件内容",
    hint: "智能整理会截取当前窗口识别文字作为参考，需要屏幕录制权限。",
    pending: "允许后要重启说吧！才生效",
  },
  {
    id: "inputMonitoring",
    kind: "input-monitoring",
    label: "Caps Lock 快捷键",
    hint: "用 Caps Lock 触发语音输入需要输入监控权限。",
    pending: "没有这项权限时 Caps Lock 快捷键无法使用",
  },
] as const;

/**
 * Mac 引导里要用户处理的系统权限：辅助功能（把文字输入其他软件）可就地申请，
 * 其余见 PRIVACY_ROWS。Windows 不需要额外授权，恒为就绪。
 */
export function useSetupPermissions() {
  const [checks, setChecks] = useState<SetupCheckResult[]>([]);
  const [requesting, setRequesting] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const setError = (id: string, message: string) => setErrors((current) => ({ ...current, [id]: message }));

  useEffect(() => {
    if (!isMacOS) return;
    const refresh = () => void cmd<SetupStatus>(CMD.getSetupStatus)
      .then((status) => setChecks(status.checks))
      .catch((reason) => setError("permissions", `读取权限状态失败：${String(reason)}`));
    refresh();
    // 授权在系统设置里完成，回到软件时重新读取。
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);

  const requestTextInput = async () => {
    setRequesting(true);
    setError("permissions", "");
    try {
      const next = await cmd<SetupCheckResult>(CMD.requestSetupPermissions);
      setChecks((current) => [...current.filter((item) => item.id !== next.id), next]);
    } catch (reason) {
      setError("permissions", `授予权限失败：${String(reason)}`);
    } finally {
      setRequesting(false);
    }
  };

  const openSettings = (id: string, kind: string) => {
    setError(id, "");
    void cmd(CMD.openPrivacySettings, { kind })
      .catch((reason) => setError(id, `打开系统设置失败：${String(reason)}`));
  };

  return {
    needed: isMacOS,
    textInputReady: !isMacOS || checks.find((item) => item.id === "permissions")?.status === "ready",
    check: (id: string) => checks.find((item) => item.id === id),
    errors,
    requesting,
    requestTextInput,
    openSettings,
  };
}
