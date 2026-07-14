import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import {
  installPluginPackage,
  refreshPluginConsumers,
  requiresExplicitTrust,
  type PluginBackup,
  type PluginSnapshot,
  type PluginSummary,
} from "@/features/plugins/pluginInstaller";
import { CMD, cmd } from "@/lib/tauri";

const TRUST_LABEL: Record<PluginSummary["trust"], string> = {
  trusted: "签名可信",
  "signed-untrusted": "签名有效，密钥未信任",
  "integrity-only": "仅完整性校验",
  unsigned: "未签名",
};

export function PluginManagerPanel() {
  const [snapshot, setSnapshot] = useState<PluginSnapshot>();
  const [backups, setBackups] = useState<PluginBackup[]>([]);
  const [message, setMessage] = useState("");
  const [busyPluginId, setBusyPluginId] = useState("");

  const loadSnapshot = async () => {
    const [next, nextBackups] = await Promise.all([
      cmd<PluginSnapshot>(CMD.listProviderPlugins),
      cmd<PluginBackup[]>(CMD.listProviderPluginBackups),
    ]);
    setSnapshot(next);
    setBackups(nextBackups);
  };

  useEffect(() => {
    void loadSnapshot().catch((error) => setMessage(`读取插件失败：${String(error)}`));
  }, []);

  const reload = async () => {
    setMessage("");
    try {
      const next = await cmd<PluginSnapshot>(CMD.reloadProviderPlugins);
      setSnapshot(next);
      await refreshPluginConsumers();
      await loadSnapshot();
      setMessage("插件目录已重新扫描。");
    } catch (error) {
      setMessage(`重新扫描失败：${String(error)}`);
    }
  };

  const installFromPath = async (sourcePath: string) => {
    setMessage("");
    try {
      const next = await installPluginPackage(sourcePath, { allowUnsigned: false, trustSigningKey: false });
      setSnapshot(next);
      await loadSnapshot();
      setMessage("插件已安装并加载。");
    } catch (error) {
      const reason = String(error);
      if (!requiresExplicitTrust(error) || !window.confirm(`${reason}\n\n确认信任此来源并继续安装吗？`)) {
        setMessage(`安装失败：${reason}`);
        return;
      }
      try {
        const next = await installPluginPackage(sourcePath, { allowUnsigned: true, trustSigningKey: true });
        setSnapshot(next);
        await loadSnapshot();
        setMessage("插件已在明确授权后安装。");
      } catch (retryError) {
        setMessage(`安装失败：${String(retryError)}`);
      }
    }
  };

  const install = async () => {
    const selected = await open({
      multiple: false,
      title: "选择说吧插件包",
      filters: [{ name: "说吧插件包", extensions: ["sayit"] }],
    });
    if (typeof selected === "string") await installFromPath(selected);
  };

  const setEnabled = async (plugin: PluginSummary, enabled: boolean) => {
    setBusyPluginId(plugin.id);
    setMessage("");
    try {
      const next = await cmd<PluginSnapshot>(CMD.setProviderPluginEnabled, { pluginId: plugin.id, enabled });
      setSnapshot(next);
      await refreshPluginConsumers();
      setMessage(`${plugin.name}已${enabled ? "启用" : "停用"}。`);
    } catch (error) {
      setMessage(`更新插件状态失败：${String(error)}`);
    } finally {
      setBusyPluginId("");
    }
  };

  const restore = async (plugin: PluginSummary) => {
    if (!window.confirm(`确认将 ${plugin.name} 恢复到最近的备份版本吗？`)) return;
    setBusyPluginId(plugin.id);
    setMessage("");
    try {
      const next = await cmd<PluginSnapshot>(CMD.rollbackProviderPlugin, { pluginId: plugin.id });
      setSnapshot(next);
      await refreshPluginConsumers();
      await loadSnapshot();
      setMessage("插件已恢复到上一版本。");
    } catch (error) {
      setMessage(`恢复上一版本失败：${String(error)}`);
    } finally {
      setBusyPluginId("");
    }
  };

  return (
    <SettingsSection title="插件管理">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
        <p className="text-sm text-[var(--color-fg-subtle)]">安装和管理识别供应商插件；登录、密钥与其他配置请在「密钥与识别」中完成。</p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="primary" onClick={() => void install()}>安装 .sayit 插件</Button>
          <Button size="sm" onClick={() => void reload()}>重新扫描</Button>
        </div>
      </div>

      <div className="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)]">
        {snapshot?.plugins.map((plugin, index) => {
          const hasBackup = backups.some((backup) => backup.pluginId === plugin.id);
          const busy = busyPluginId === plugin.id;
          return (
            <div
              key={plugin.id}
              className={index ? "border-t border-[var(--color-line)] px-4 py-3" : "px-4 py-3"}
            >
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                    <p className="text-sm font-medium text-[var(--color-fg)]">{plugin.name}</p>
                    <span className="text-xs text-[var(--color-fg-subtle)]">v{plugin.version}</span>
                    <span className="text-xs text-[var(--color-fg-subtle)]">{TRUST_LABEL[plugin.trust]}</span>
                  </div>
                  <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
                    权限：{plugin.permissions.join("、") || "无"}
                  </p>
                </div>
                {hasBackup && (
                  <Button size="sm" disabled={busy} onClick={() => void restore(plugin)}>恢复上一版本</Button>
                )}
                <div className="flex items-center gap-2">
                  <span className="text-xs text-[var(--color-fg-subtle)]">{plugin.enabled ? "已启用" : "已停用"}</span>
                  <Switch
                    checked={plugin.enabled}
                    disabled={busy}
                    label={`${plugin.name}${plugin.enabled ? "已启用，点击停用" : "已停用，点击启用"}`}
                    onChange={(enabled) => void setEnabled(plugin, enabled)}
                  />
                </div>
              </div>
            </div>
          );
        })}
        {snapshot && snapshot.plugins.length === 0 && (
          <p className="px-4 py-7 text-center text-sm text-[var(--color-fg-subtle)]">尚未安装插件。可点击上方按钮，或将 .sayit 文件拖入应用安装。</p>
        )}
      </div>

      {snapshot?.errors.map((error) => (
        <p key={error.path} className="text-xs text-[var(--color-err)]">{error.path}：{error.message}</p>
      ))}
      {message && <p className="text-sm text-[var(--color-fg-subtle)]">{message}</p>}
    </SettingsSection>
  );
}
