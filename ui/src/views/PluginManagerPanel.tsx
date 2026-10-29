import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Download, Trash2 } from "lucide-react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Group } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { Modal } from "@/components/ui/Modal";
import { Tooltip } from "@/components/ui/Tooltip";
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
  superseded: "已内置，可卸载",
  pending: "等待下载",
  partial: "下载未完成",
  corrupt: "模型校验失败",
  downloading: "正在下载",
  ready: "模型已就绪",
};

const RESIDENCY_MEMORY: Record<string, string> = {
  "official.sherpa-funasr-nano-ctc": "约 400 MB",
  "official.sherpa-sensevoice-small": "约 350 MB",
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

  const setKeepLoaded = async (plugin: PluginSummary, keepLoaded: boolean) => {
    setBusyPluginId(plugin.id);
    setMessage("");
    try {
      setSnapshot(await cmd<PluginSnapshot>(CMD.setProviderModelKeepLoaded, { pluginId: plugin.id, keepLoaded }));
    } catch (error) {
      setMessage(`更新常驻内存失败：${String(error)}`);
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
    <InspectorBody>
      <Group
        title="插件管理"
        description="安装并启用插件或模型包后，就能在相应功能中选择其中的模型。"
        right={
          <>
            <Button size="sm" disabled={scanning} onClick={() => void reload()}>
              {scanning ? "扫描中…" : "重新扫描"}
            </Button>
            <Button size="sm" variant="primary" disabled={scanning} onClick={() => void install()}>安装 .sayit 包</Button>
          </>
        }
      >
        {snapshot?.plugins.map((plugin) => {
          const busy = busyPluginId === plugin.id;
          return (
            <div key={plugin.id} className="px-3.5 py-3">
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                    <p className="text-[13px] font-medium text-[var(--color-fg)]">{plugin.name}</p>
                    <span className="text-xs text-[var(--color-fg-subtle)]">v{plugin.version}</span>
                    <span className="text-xs text-[var(--color-fg-subtle)]">{plugin.runtimeKind === "model-pack" ? "模型包" : "连接器"}</span>
                    <span className="text-xs text-[var(--color-fg-subtle)]">{TRUST_LABEL[plugin.trust]}</span>
                  </div>
                  <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
                    权限：{plugin.permissions.map((permission) => PERMISSION_LABEL[permission] || permission).join("、") || "无"}
                  </p>
                  {plugin.modelPack && (
                    <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
                      {MODEL_STATE_LABEL[plugin.modelPack.state] || plugin.modelPack.state}{plugin.modelPack.state !== "superseded" && ` · ${formatBytes(plugin.modelPack.readyBytes)} / ${formatBytes(plugin.modelPack.totalBytes)}`}
                    </p>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  {plugin.modelPack?.downloadable && plugin.modelPack.state !== "ready" && (
                    <Button size="sm" disabled={busy} onClick={() => void downloadModel(plugin)}>
                      <Download className="h-3.5 w-3.5" aria-hidden />
                      {plugin.modelPack.state === "partial" ? "继续下载" : "下载模型"}
                    </Button>
                  )}
                  {plugin.modelPack?.residency && (
                    <Tooltip content={`开启后模型一直留在内存里，听写和字幕不用再等加载；文件识别单独加载。会持续占用${RESIDENCY_MEMORY[plugin.id] || "数百 MB"}内存，处理长音频时可能更多。`}>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-[var(--color-fg-subtle)]">常驻内存</span>
                        <Switch
                          checked={plugin.modelPack.keepLoaded}
                          disabled={busy || (!plugin.modelPack.keepLoaded && (!plugin.enabled || plugin.modelPack.state !== "ready"))}
                          label={`${plugin.name}常驻内存`}
                          onChange={(keepLoaded) => void setKeepLoaded(plugin, keepLoaded)}
                        />
                      </div>
                    </Tooltip>
                  )}
                  {plugin.modelPack?.state !== "superseded" && (
                    <>
                      <span className="text-xs text-[var(--color-fg-subtle)]">{plugin.enabled ? "已启用" : "已停用"}</span>
                      <Switch
                        checked={plugin.enabled}
                        disabled={busy}
                        label={`${plugin.name}${plugin.enabled ? "已启用，点击停用" : "已停用，点击启用"}`}
                        onChange={(enabled) => void setEnabled(plugin, enabled)}
                      />
                    </>
                  )}
                  <IconButton
                    size="sm"
                    variant="dangerHover"
                    disabled={busy}
                    label={`卸载${plugin.name}`}
                    onClick={() => setPendingUninstall(plugin)}
                  >
                    <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  </IconButton>
                </div>
              </div>
              {plugin.modelPack?.keepLoaded && plugin.modelPack.residency?.state === "loading" && (
                <p role="status" className="mt-1 text-xs text-[var(--color-fg-subtle)]">正在加载到内存…</p>
              )}
              {plugin.modelPack?.residency?.state === "loaded" && (
                <p role="status" className="mt-1 text-xs text-[var(--color-fg-subtle)]">已常驻内存</p>
              )}
              {plugin.modelPack?.residency?.state === "failed" && (
                <p role="alert" className="mt-1 text-xs text-[var(--color-err)]">常驻加载失败：{plugin.modelPack.residency.error}</p>
              )}
            </div>
          );
        })}
        {(!snapshot || snapshot.plugins.length === 0) && (
          <p className="px-3.5 py-7 text-center text-[13px] text-[var(--color-fg-subtle)]">
            {snapshot ? "尚未安装插件。可点击上方按钮，或将 .sayit 文件拖入应用安装。" : message ? "插件列表读取失败" : "正在读取插件…"}
          </p>
        )}
      </Group>

      {snapshot?.errors.map((error) => (
        <p key={error.path} className="text-xs text-[var(--color-err)]">{error.path}：{error.message}</p>
      ))}
      {message && <p className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
      <Modal
        open={Boolean(pendingUninstall)}
        onClose={() => !busyPluginId && setPendingUninstall(undefined)}
        title="确认卸载插件"
        showCloseButton={false}
        className="max-w-[430px]"
      >
        <div className="p-5">
          <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            确认卸载“{pendingUninstall?.name}”吗？{pendingUninstall?.runtimeKind === "model-pack"
              ? "模型文件与该模型包的设置会被删除。"
              : "插件代码、登录会话、Cookie 与浏览数据会被删除；非敏感配置和本地加密凭据会保留，重新安装后可继续使用。"}
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button size="sm" variant="dangerHover" disabled={Boolean(busyPluginId)} onClick={() => pendingUninstall && void uninstall(pendingUninstall)}>
              {busyPluginId ? "正在卸载..." : "卸载"}
            </Button>
            <Button size="sm" variant="primary" autoFocus disabled={Boolean(busyPluginId)} onClick={() => setPendingUninstall(undefined)}>取消</Button>
          </div>
        </div>
      </Modal>
    </InspectorBody>
  );
}
