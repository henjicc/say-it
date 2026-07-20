import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Download, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Modal } from "@/components/ui/Modal";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import {
  refreshPluginConsumers,
  type PluginSnapshot,
  type PluginSummary,
} from "@/features/plugins/pluginInstaller";
import { CMD, EVT, cmd, on } from "@/lib/tauri";
import { usePluginImportStore } from "@/store/usePluginImportStore";

const TRUST_LABEL: Record<PluginSummary["trust"], string> = {
  trusted: "签名可信",
  "signed-untrusted": "签名有效，密钥未信任",
  "integrity-only": "仅完整性校验",
  unsigned: "未签名",
};

const PERMISSION_LABEL: Record<string, string> = {
  network: "网络",
  localNetwork: "本机网络",
  browserSession: "浏览器会话",
  cookies: "Cookie",
};

const MODEL_STATE_LABEL: Record<string, string> = {
  pending: "等待下载",
  partial: "下载未完成",
  corrupt: "模型校验失败",
  downloading: "正在下载",
  ready: "模型已就绪",
};

function formatBytes(value: number) {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(0, value / 1024).toFixed(1)} KB`;
}

export function PluginManagerPanel() {
  const [snapshot, setSnapshot] = useState<PluginSnapshot>();
  const [message, setMessage] = useState("");
  const [busyPluginId, setBusyPluginId] = useState("");
  const [scanning, setScanning] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<PluginSummary>();
  const enqueueImport = usePluginImportStore((state) => state.enqueue);

  const loadSnapshot = async () => {
    const next = await cmd<PluginSnapshot>(CMD.listProviderPlugins);
    setSnapshot(next);
  };

  useEffect(() => {
    void loadSnapshot().catch((error) => setMessage(`读取插件失败：${String(error)}`));
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keepListener = (stop: () => void) => {
      if (disposed) stop();
      else unlisteners.push(stop);
    };
    void on<{
      pluginId: string;
      packDownloadedBytes: number;
      packTotalBytes: number;
      state: string;
    }>(EVT.modelPackProgress, (event) => {
      setSnapshot((current) => current && ({
        ...current,
        plugins: current.plugins.map((plugin) => plugin.id === event.pluginId && plugin.modelPack
          ? { ...plugin, modelPack: { ...plugin.modelPack, state: event.state as "downloading" | "ready", readyBytes: event.packDownloadedBytes, totalBytes: event.packTotalBytes } }
          : plugin),
      }));
    }).then(keepListener);
    void on<{ extractedBytes: number; totalBytes: number }>(EVT.pluginInstallProgress, (event) => {
      setMessage(`正在安装：${formatBytes(event.extractedBytes)} / ${formatBytes(event.totalBytes)}`);
    }).then(keepListener);
    void on(EVT.pluginRegistryChanged, () => {
      void loadSnapshot().catch((error) => setMessage(`刷新插件列表失败：${String(error)}`));
    }).then(keepListener);
    return () => {
      disposed = true;
      unlisteners.forEach((stop) => stop());
    };
  }, []);

  const reload = async () => {
    // 重新扫描会对已安装文件逐个复核 SHA256，模型包动辄数百 MB，耗时以秒计。
    // 没有进行中状态时界面完全无反馈，用户会以为软件卡死。
    if (scanning) return;
    setScanning(true);
    setMessage("正在扫描插件目录并校验文件完整性…");
    try {
      const next = await cmd<PluginSnapshot>(CMD.reloadProviderPlugins);
      setSnapshot(next);
      await refreshPluginConsumers();
      await loadSnapshot();
      setMessage("插件目录已重新扫描。");
    } catch (error) {
      setMessage(`重新扫描失败：${String(error)}`);
    } finally {
      setScanning(false);
    }
  };

  const install = async () => {
    const selected = await open({
      multiple: false,
      title: "选择说吧插件或模型包",
      filters: [{ name: "说吧 .sayit 包", extensions: ["sayit"] }],
    });
    if (typeof selected === "string") enqueueImport([selected]);
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

  const downloadModel = async (plugin: PluginSummary) => {
    setBusyPluginId(plugin.id);
    setMessage("");
    try {
      const next = await cmd<PluginSnapshot>(CMD.downloadProviderModelPack, { pluginId: plugin.id });
      setSnapshot(next);
      await refreshPluginConsumers();
      setMessage(`${plugin.name}模型已下载并校验。`);
    } catch (error) {
      setMessage(`模型下载失败：${String(error)}`);
      await loadSnapshot();
    } finally {
      setBusyPluginId("");
    }
  };

  const uninstall = async (plugin: PluginSummary) => {
    setBusyPluginId(plugin.id);
    setMessage("");
    try {
      const next = await cmd<PluginSnapshot>(CMD.uninstallProviderPlugin, { pluginId: plugin.id });
      setSnapshot(next);
      await refreshPluginConsumers();
      await loadSnapshot();
      setMessage("插件已卸载。");
      setPendingUninstall(undefined);
    } catch (error) {
      setMessage(`卸载失败：${String(error)}`);
    } finally {
      setBusyPluginId("");
    }
  };

  return (
    <SettingsSection title="插件管理">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
        <p className="text-sm text-[var(--color-fg-subtle)]">安装和管理供应商插件与本地模型包；安装并启用后，对应模型会自动出现在各场景的模型下拉框。</p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="primary" disabled={scanning} onClick={() => void install()}>安装 .sayit 包</Button>
          <Button size="sm" disabled={scanning} onClick={() => void reload()}>
            {scanning ? "扫描中…" : "重新扫描"}
          </Button>
        </div>
      </div>

      <div className="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)]">
        {snapshot?.plugins.map((plugin, index) => {
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
                    <span className="text-xs text-[var(--color-fg-subtle)]">{plugin.runtimeKind === "model-pack" ? "模型包" : "连接器"}</span>
                    <span className="text-xs text-[var(--color-fg-subtle)]">{TRUST_LABEL[plugin.trust]}</span>
                  </div>
                  <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
                    权限：{plugin.permissions.map((permission) => PERMISSION_LABEL[permission] || permission).join("、") || "无"}
                  </p>
                  {plugin.modelPack && (
                    <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
                      {MODEL_STATE_LABEL[plugin.modelPack.state] || plugin.modelPack.state} · {formatBytes(plugin.modelPack.readyBytes)} / {formatBytes(plugin.modelPack.totalBytes)}
                    </p>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  {plugin.modelPack?.downloadable && plugin.modelPack.state !== "ready" && (
                    <Button size="sm" disabled={busy} onClick={() => void downloadModel(plugin)}>
                      <Download className="h-4 w-4" aria-hidden />
                      {plugin.modelPack.state === "partial" ? "继续下载" : "下载模型"}
                    </Button>
                  )}
                  <span className="text-xs text-[var(--color-fg-subtle)]">{plugin.enabled ? "已启用" : "已停用"}</span>
                  <Switch
                    checked={plugin.enabled}
                    disabled={busy}
                    label={`${plugin.name}${plugin.enabled ? "已启用，点击停用" : "已停用，点击启用"}`}
                    onChange={(enabled) => void setEnabled(plugin, enabled)}
                  />
                  <IconButton
                    size="sm"
                    variant="dangerHover"
                    disabled={busy}
                    label={`卸载${plugin.name}`}
                    onClick={() => setPendingUninstall(plugin)}
                  >
                    <Trash2 className="h-4 w-4" strokeWidth={1.8} aria-hidden />
                  </IconButton>
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
      <Modal
        open={Boolean(pendingUninstall)}
        onClose={() => !busyPluginId && setPendingUninstall(undefined)}
        title="确认卸载插件"
        showCloseButton={false}
        className="max-w-[430px]"
      >
        <div className="p-5">
          <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            确认卸载“{pendingUninstall?.name}”吗？插件配置、登录会话、Cookie、浏览数据与已下载的模型文件都会一并删除，无法恢复。
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button size="sm" variant="dangerHover" disabled={Boolean(busyPluginId)} onClick={() => pendingUninstall && void uninstall(pendingUninstall)}>
              {busyPluginId ? "正在卸载..." : "卸载"}
            </Button>
            <Button size="sm" variant="primary" autoFocus disabled={Boolean(busyPluginId)} onClick={() => setPendingUninstall(undefined)}>取消</Button>
          </div>
        </div>
      </Modal>
    </SettingsSection>
  );
}
