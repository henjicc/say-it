import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Collapse } from "@/components/ui/Collapse";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";

interface PluginSummary {
  id: string;
  name: string;
  version: string;
  providerId: string;
  permissions: string[];
  models: string[];
  trust: "trusted" | "signed-untrusted" | "integrity-only" | "unsigned";
  actions: string[];
  hasBrowserSession: boolean;
}

interface PluginSnapshot {
  apiVersion: number;
  plugins: PluginSummary[];
  errors: { path: string; message: string }[];
}

const TRUST_LABEL: Record<PluginSummary["trust"], string> = {
  trusted: "签名可信",
  "signed-untrusted": "签名有效，密钥未信任",
  "integrity-only": "仅完整性校验",
  unsigned: "未签名",
};

export function PluginManagerPanel() {
  const loadProviders = useProviderStore((state) => state.load);
  const [snapshot, setSnapshot] = useState<PluginSnapshot>();
  const [message, setMessage] = useState("");

  const reload = async () => {
    const next = await cmd<PluginSnapshot>(CMD.reloadProviderPlugins);
    setSnapshot(next);
    await loadProviders();
  };

  useEffect(() => {
    void cmd<PluginSnapshot>(CMD.listProviderPlugins).then(setSnapshot).catch((error) => {
      setMessage(`读取插件失败：${String(error)}`);
    });
  }, []);

  const install = async () => {
    const selected = await open({
      multiple: false,
      title: "选择说吧包",
      filters: [{ name: "说吧包", extensions: ["sayit"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    try {
      const next = await cmd<PluginSnapshot>(CMD.installProviderPlugin, {
        sourcePath: selected,
        allowUnsigned: false,
        trustSigningKey: false,
      });
      setSnapshot(next);
      await loadProviders();
      setMessage("插件已安装并加载。");
    } catch (error) {
      const reason = String(error);
      const canOverride = reason.includes("未签名") || reason.includes("尚未受信任");
      if (!canOverride || !window.confirm(`${reason}\n\n确认信任此来源并继续安装吗？`)) {
        setMessage(`安装失败：${reason}`);
        return;
      }
      try {
        const next = await cmd<PluginSnapshot>(CMD.installProviderPlugin, {
          sourcePath: selected,
          allowUnsigned: true,
          trustSigningKey: true,
        });
        setSnapshot(next);
        await loadProviders();
        setMessage("插件已在明确授权后安装。");
      } catch (retryError) {
        setMessage(`安装失败：${String(retryError)}`);
      }
    }
  };

  const rollback = async (plugin: PluginSummary) => {
    if (!window.confirm(`确认把 ${plugin.name} 回滚到最近的备份版本吗？`)) return;
    try {
      const next = await cmd<PluginSnapshot>(CMD.rollbackProviderPlugin, { pluginId: plugin.id });
      setSnapshot(next);
      await loadProviders();
      setMessage("插件已回滚。");
    } catch (error) {
      setMessage(`回滚失败：${String(error)}`);
    }
  };

  return (
    <Collapse title="插件管理" subtitle={`宿主 API v${snapshot?.apiVersion ?? 2}`}>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" onClick={() => void install()}>安装 .sayit 包</Button>
        <Button size="sm" onClick={() => void reload()}>重新扫描</Button>
      </div>
      <div className="mt-3 flex flex-col gap-2">
        {(snapshot?.plugins || []).map((plugin) => (
          <div key={plugin.id} className="rounded-[var(--radius-md)] bg-[var(--color-bg)] px-3 py-2.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <p className="text-sm text-[var(--color-fg)]">{plugin.name} · {plugin.version}</p>
                <p className="text-xs text-[var(--color-fg-subtle)]">
                  {TRUST_LABEL[plugin.trust]} · 权限：{plugin.permissions.join("、") || "无"}
                </p>
              </div>
              <Button size="sm" onClick={() => void rollback(plugin)}>回滚</Button>
            </div>
          </div>
        ))}
        {snapshot?.errors.map((error) => (
          <p key={error.path} className="text-xs text-[var(--color-danger)]">
            {error.path}：{error.message}
          </p>
        ))}
      </div>
      {message && <p className="mt-3 text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </Collapse>
  );
}
