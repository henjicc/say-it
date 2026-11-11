import { useEffect, useState } from "react";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd, type SetupCheckResult, type SetupStatus } from "@/lib/tauri";

/** Mac 需要辅助功能权限才能把文字输入其他软件；Windows 不需要额外授权，恒为就绪。 */
export function useTextInputPermission() {
  const [check, setCheck] = useState<SetupCheckResult>();
  const [requesting, setRequesting] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!isMacOS) return;
    const refresh = () => void cmd<SetupStatus>(CMD.getSetupStatus)
      .then((status) => setCheck(status.checks.find((item) => item.id === "permissions")))
      .catch((reason) => setError(`读取权限状态失败：${String(reason)}`));
    refresh();
    // 授权在系统设置里完成，回到软件时重新读取。
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);

  const request = async () => {
    setRequesting(true);
    setError("");
    try {
      setCheck(await cmd<SetupCheckResult>(CMD.requestSetupPermissions));
    } catch (reason) {
      setError(`授予权限失败：${String(reason)}`);
    } finally {
      setRequesting(false);
    }
  };

  return { needed: isMacOS, ready: !isMacOS || check?.status === "ready", requesting, error, request };
}
