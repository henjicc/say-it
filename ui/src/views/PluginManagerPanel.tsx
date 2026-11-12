import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Download, Trash2 } from "lucide-react";
import { Meter } from "@/components/ui/Meter";
import { InspectorBody, InspectorHeader } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { CheckField } from "@/components/ui/Field";
import { Group } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { Modal } from "@/components/ui/Modal";
import { Pill } from "@/components/ui/Pill";
import { HelpLabel, Tooltip } from "@/components/ui/Tooltip";
import { Switch } from "@/components/ui/Switch";
import {
  refreshPluginConsumers,
  type OfficialModelPack,
  type PluginSnapshot,
  type PluginSummary,
} from "@/features/plugins/pluginInstaller";
import { hardwareFit, hardwareLabel, LOCAL_MODEL_NEEDS, recommendedAsrPack, useHardware, type HardwareSummary } from "@/features/models/localModelNeeds";
import { CMD, EVT, cmd, on } from "@/lib/tauri";
import { useLocalModels } from "@/features/models/localModels";
import { usePluginImportStore } from "@/store/usePluginImportStore";
import { notify } from "@/store/useToastStore";

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
};

/** 按点分数字段比较版本号，只有目录版本更高才算有更新。官方模型包只用纯数字版本。 */
function isNewerVersion(candidate: string, installed: string) {
  const left = candidate.split(".").map(Number);
  const right = installed.split(".").map(Number);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  return false;
}

function formatBytes(value: number) {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(0, value / 1024).toFixed(1)} KB`;
}

/**
 * 插件与模型包的各项操作；快照和模型下载状态在 useLocalModels 里，与引导共用，
 * 下载期间切走再回来也能接上进度。withOfficialCatalog 时同时读取官方模型目录。
 */
function usePluginManager(withOfficialCatalog: boolean) {
  const snapshot = useLocalModels((state) => state.snapshot);
  const loadError = useLocalModels((state) => state.loadError);
  const packsError = useLocalModels((state) => state.packsError);
  const officialPacks = useLocalModels((state) => state.packs);
  const downloaded = useLocalModels((state) => state.downloaded);
  const installing = useLocalModels((state) => state.installing);
  const installError = useLocalModels((state) => state.installError);
  const installPacks = useLocalModels((state) => state.install);
  const [busyPluginId, setBusyPluginId] = useState("");
  const [scanning, setScanning] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<PluginSummary>();
  /** 启用、常驻、卸载失败的原因，显示在对应那一行下面；下载失败在 installError 里。 */
  const [rowErrors, setRowErrors] = useState<Record<string, string>>({});
  const enqueueImport = usePluginImportStore((state) => state.enqueue);
  const setSnapshot = (next: PluginSnapshot) => useLocalModels.setState({ snapshot: next });
  const { refresh, load } = useLocalModels.getState();
  const loadSnapshot = withOfficialCatalog ? load : refresh;

  const clearRowError = (id: string) => setRowErrors(({ [id]: _, ...rest }) => rest);
  const failRow = (id: string, error: unknown) => setRowErrors((current) => ({ ...current, [id]: String(error) }));

  useEffect(() => {
    void loadSnapshot().catch((error) => useLocalModels.setState({ loadError: `读取插件列表失败：${String(error)}` }));
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void on(EVT.pluginRegistryChanged, () => {
      void refresh().catch((error) => notify(`刷新插件列表失败：${String(error)}`, "err"));
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const reload = async () => {
    // 重新扫描会对已安装文件逐个复核 SHA256，模型包动辄数百 MB，耗时以秒计，按钮在此期间显示进行中。
    if (scanning) return;
    setScanning(true);
    setRowErrors({});
    useLocalModels.setState({ installError: undefined });
    try {
      setSnapshot(await cmd<PluginSnapshot>(CMD.reloadProviderPlugins));
      await refreshPluginConsumers();
      await refresh();
      notify("已重新扫描");
    } catch (error) {
      notify(`重新扫描失败：${String(error)}`, "err");
    } finally {
      setScanning(false);
    }
  };

  /** 这一行的下载进度（0～1）；没有在下载时为空。 */
  const downloadProgress = (id: string, totalBytes: number): number | undefined => {
    const pack = snapshot?.plugins.find((plugin) => plugin.id === id)?.modelPack;
    if (!installing.includes(id) && pack?.state !== "downloading") return undefined;
    // 窗口重建后还没收到进度事件时，先用快照里已就绪的字节数。
    const done = downloaded[id] ?? pack?.readyBytes ?? 0;
    return totalBytes ? Math.min(1, done / totalBytes) : 0;
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
    clearRowError(plugin.id);
    try {
      const next = await cmd<PluginSnapshot>(CMD.setProviderPluginEnabled, { pluginId: plugin.id, enabled });
      setSnapshot(next);
      await refreshPluginConsumers();
    } catch (error) {
      failRow(plugin.id, `更新插件状态失败：${String(error)}`);
    } finally {
      setBusyPluginId("");
    }
  };

  const setKeepLoaded = async (plugin: PluginSummary, keepLoaded: boolean) => {
    setBusyPluginId(plugin.id);
    clearRowError(plugin.id);
    try {
      setSnapshot(await cmd<PluginSnapshot>(CMD.setProviderModelKeepLoaded, { pluginId: plugin.id, keepLoaded }));
    } catch (error) {
      failRow(plugin.id, `更新常驻内存失败：${String(error)}`);
    } finally {
      setBusyPluginId("");
    }
  };

  const uninstall = async (plugin: PluginSummary) => {
    setBusyPluginId(plugin.id);
    clearRowError(plugin.id);
    try {
      setSnapshot(await cmd<PluginSnapshot>(CMD.uninstallProviderPlugin, { pluginId: plugin.id }));
      await refreshPluginConsumers();
      await refresh();
    } catch (error) {
      failRow(plugin.id, `卸载失败：${String(error)}`);
      await refresh();
    } finally {
      setBusyPluginId("");
      setPendingUninstall(undefined);
    }
  };

  return {
    snapshot, loadError, packsError, busyPluginId, scanning, rowErrors, officialPacks, pendingUninstall, setPendingUninstall,
    reload, install, setEnabled, setKeepLoaded, uninstall, downloadProgress,
    /** 同一时刻只下载一批；下载期间其他下载按钮不可用。 */
    downloadBusy: installing.length > 0 || scanning,
    downloadError: (id: string) => (installError?.ids.includes(id) ? installError.message : undefined),
    download: (id: string, update = false) => void installPacks([id], update),
    retryLoad: load,
  };
}

type Manager = ReturnType<typeof usePluginManager>;

/** 模型在这台电脑上合不合适：首选的标「推荐」，跑不动的写明原因，其余不出字。 */
function ModelFit({ pluginId, hardware }: { pluginId: string; hardware: HardwareSummary | undefined }) {
  if (recommendedAsrPack(hardware) === pluginId) return <Pill className="px-2 py-px text-[11px]">推荐</Pill>;
  const fit = hardwareFit(LOCAL_MODEL_NEEDS[pluginId], hardware);
  return fit && !fit.ok ? <span className="text-xs text-[var(--color-warn)]">{fit.text}</span> : null;
}

/** 下载中的进度条，切到别的页面再回来也接得上。 */
function DownloadMeter({ name, progress, totalBytes }: { name: string; progress: number; totalBytes: number }) {
  return (
    <div className="flex w-56 items-center gap-2.5">
      <Meter value={progress} label={`${name}下载进度`} className="flex-1" />
      <span className="flex-none text-xs tabular-nums text-[var(--color-fg-subtle)]">
        {totalBytes ? `${formatBytes(progress * totalBytes)} / ${formatBytes(totalBytes)}` : "准备中…"}
      </span>
    </div>
  );
}

function RowError({ text }: { text?: string }) {
  return text ? <p role="alert" className="mt-1 break-words text-xs text-[var(--color-err)]">{text}</p> : null;
}

/** 已安装的一行：模型包或连接器插件。update 是官方目录里更新的版本。 */
function InstalledRow({ plugin, manager, hardware, update }: {
  plugin: PluginSummary;
  manager: Manager;
  hardware?: HardwareSummary;
  update?: OfficialModelPack;
}) {
  const busy = manager.busyPluginId === plugin.id;
  const pack = plugin.modelPack;
  const totalBytes = update?.sizeBytes ?? pack?.totalBytes ?? 0;
  const progress = pack && manager.downloadProgress(plugin.id, totalBytes);
  const downloadLabel = pack?.state === "partial" ? "继续下载" : "下载模型";
  return (
    <div className="px-3.5 py-3">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <p className="text-[13px] font-medium text-[var(--color-fg)]">
              <HelpLabel content={LOCAL_MODEL_NEEDS[plugin.id]?.summary}>{plugin.name}</HelpLabel>
            </p>
            <span className="text-xs text-[var(--color-fg-subtle)]">{update ? `v${plugin.version} → v${update.version}` : `v${plugin.version}`}</span>
            {pack?.state === "ready" && <span className="text-xs text-[var(--color-fg-subtle)]">{formatBytes(pack.totalBytes)}</span>}
            {plugin.trust !== "trusted" && <span className="text-xs text-[var(--color-warn)]">{TRUST_LABEL[plugin.trust]}</span>}
            <ModelFit pluginId={plugin.id} hardware={hardware} />
          </div>
          {plugin.permissions.length > 0 && (
            <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
              权限：{plugin.permissions.map((permission) => PERMISSION_LABEL[permission] || permission).join("、")}
            </p>
          )}
          {pack && pack.state !== "ready" && progress === undefined && (
            <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">
              {MODEL_STATE_LABEL[pack.state] || pack.state}{pack.state !== "superseded" && ` · ${formatBytes(pack.readyBytes)} / ${formatBytes(pack.totalBytes)}`}
            </p>
          )}
        </div>
        <div className="flex items-center gap-2">
          {progress !== undefined && <DownloadMeter name={plugin.name} progress={progress} totalBytes={totalBytes} />}
          {progress === undefined && update && (
            <Button size="sm" aria-label={`更新${plugin.name}`} disabled={manager.downloadBusy || busy} onClick={() => manager.download(plugin.id, true)}>
              <Download className="h-3.5 w-3.5" aria-hidden />
              更新
            </Button>
          )}
          {progress === undefined && !update && pack?.downloadable && pack.state !== "ready" && (
            <Button size="sm" aria-label={`${downloadLabel}${plugin.name}`} disabled={manager.downloadBusy || busy} onClick={() => manager.download(plugin.id)}>
              <Download className="h-3.5 w-3.5" aria-hidden />
              {downloadLabel}
            </Button>
          )}
          {pack?.residency && (
            <Tooltip content={pack.engine === "sherpa-onnx-punctuation"
              ? "开启后标点模型一直留在内存里，每次听写不用再等加载。"
              : "开启后模型一直留在内存里，听写和字幕不用再等加载；文件识别单独加载。处理长音频时占用会更多。关闭时，模型用过后还会保留几分钟，方便连续听写。"}>
              {/* 用复选框而不是开关：右侧紧挨着的开关是「启用」，两个开关并排分不清各管什么。 */}
              <span>
                <CheckField
                  className="text-xs"
                  checked={pack.keepLoaded}
                  disabled={busy || (!pack.keepLoaded && (!plugin.enabled || pack.state !== "ready"))}
                  onChange={(keepLoaded) => void manager.setKeepLoaded(plugin, keepLoaded)}
                >
                  常驻内存
                </CheckField>
              </span>
            </Tooltip>
          )}
          {pack?.state !== "superseded" && (
            <Switch
              checked={plugin.enabled}
              disabled={busy}
              label={`启用${plugin.name}`}
              onChange={(enabled) => void manager.setEnabled(plugin, enabled)}
            />
          )}
          <IconButton
            size="sm"
            variant="dangerHover"
            disabled={busy}
            label={`卸载${plugin.name}`}
            onClick={() => manager.setPendingUninstall(plugin)}
          >
            <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
          </IconButton>
        </div>
      </div>
      {pack?.keepLoaded && pack.residency?.state === "loading" && (
        <p role="status" className="mt-1 text-xs text-[var(--color-fg-subtle)]">正在加载到内存…</p>
      )}
      {pack?.keepLoaded && plugin.enabled && pack.state === "ready" && pack.residency?.state === "unloaded" && (
        <p role="status" className="mt-1 text-xs text-[var(--color-fg-subtle)]">没有功能在用这个模型，暂不加载</p>
      )}
      {pack?.residency?.state === "failed" && (
        <p role="alert" className="mt-1 text-xs text-[var(--color-err)]">常驻加载失败：{pack.residency.error}</p>
      )}
      <RowError text={manager.rowErrors[plugin.id] ?? manager.downloadError(plugin.id)} />
    </div>
  );
}

/** 官方目录里还没安装的一行。 */
function AvailableRow({ pack, manager, hardware }: { pack: OfficialModelPack; manager: Manager; hardware?: HardwareSummary }) {
  const progress = manager.downloadProgress(pack.id, pack.sizeBytes);
  const need = LOCAL_MODEL_NEEDS[pack.id]?.summary;
  return (
    <div className="px-3.5 py-3">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1">
          <p className="text-[13px] font-medium text-[var(--color-fg)]">
            <HelpLabel content={need}>{pack.name}</HelpLabel>
          </p>
          <span className="text-xs text-[var(--color-fg-subtle)]">{formatBytes(pack.sizeBytes)}</span>
          <ModelFit pluginId={pack.id} hardware={hardware} />
          <p className="basis-full text-xs text-[var(--color-fg-subtle)]">{pack.description}</p>
        </div>
        {progress !== undefined
          ? <DownloadMeter name={pack.name} progress={progress} totalBytes={pack.sizeBytes} />
          : (
            <Button size="sm" aria-label={`下载${pack.name}`} disabled={manager.downloadBusy} onClick={() => manager.download(pack.id)}>
              <Download className="h-3.5 w-3.5" aria-hidden />
              下载
            </Button>
          )}
      </div>
      <RowError text={manager.downloadError(pack.id)} />
    </div>
  );
}

/** 两页共用的收尾：扫描时发现的坏包，以及卸载确认。 */
function ManagerFooter({ manager }: { manager: Manager }) {
  const { pendingUninstall, busyPluginId, snapshot } = manager;
  return (
    <>
      {snapshot?.errors.map((error) => (
        <p key={error.path} role="alert" className="break-words text-xs text-[var(--color-err)]">{error.path}：{error.message}</p>
      ))}
      <Modal
        open={Boolean(pendingUninstall)}
        onClose={() => !busyPluginId && manager.setPendingUninstall(undefined)}
        title={pendingUninstall?.runtimeKind === "model-pack" ? "确认卸载模型" : "确认卸载插件"}
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
            <Button size="sm" variant="dangerHover" disabled={Boolean(busyPluginId)} onClick={() => pendingUninstall && void manager.uninstall(pendingUninstall)}>
              {busyPluginId ? "正在卸载..." : "卸载"}
            </Button>
            <Button size="sm" variant="primary" autoFocus disabled={Boolean(busyPluginId)} onClick={() => manager.setPendingUninstall(undefined)}>取消</Button>
          </div>
        </div>
      </Modal>
    </>
  );
}

function PackageActions({ manager, primary }: { manager: Manager; primary?: boolean }) {
  return (
    <>
      <Button size="sm" disabled={manager.scanning} onClick={() => void manager.reload()}>
        {manager.scanning ? "扫描中…" : "重新扫描"}
      </Button>
      <Button size="sm" variant={primary ? "primary" : undefined} disabled={manager.scanning} onClick={() => void manager.install()}>安装 .sayit 包</Button>
    </>
  );
}

function ListStatus({ manager, empty }: { manager: Manager; empty: string }) {
  const { snapshot, loadError } = manager;
  return (
    <p role={loadError ? "alert" : undefined} className={`px-3.5 py-7 text-center text-[13px] break-words ${loadError ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]"}`}>
      {snapshot ? empty : loadError || "正在读取…"}
    </p>
  );
}

/** 本地模型：官方目录与已安装的模型包合成一份列表，条目位置固定，只变状态。 */
export function LocalModelsPanel() {
  const manager = usePluginManager(true);
  const hardware = useHardware();
  const hardwareText = hardwareLabel(hardware);
  const { snapshot, officialPacks } = manager;
  const officialError = manager.packsError;
  const installedPacks = snapshot?.plugins.filter((plugin) => plugin.runtimeKind === "model-pack") ?? [];
  const catalog = officialPacks ?? [];
  const outsideCatalog = installedPacks.filter((plugin) => !catalog.some((pack) => pack.id === plugin.id));
  const empty = catalog.length === 0 && outsideCatalog.length === 0;
  // 目录到齐（或确定读不到）后再列出，已装的模型不会先排在后面再跳回目录里的位置。
  const listReady = Boolean(snapshot) && (Boolean(officialPacks) || Boolean(officialError));

  return (
    <>
      <InspectorHeader
        title={<HelpLabel content={hardwareText ? `这台电脑：${hardwareText}` : undefined}>本地模型</HelpLabel>}
        right={<PackageActions manager={manager} />}
      />
      <InspectorBody>
      <Group>
        {listReady && catalog.map((pack) => {
          const installed = installedPacks.find((plugin) => plugin.id === pack.id);
          return installed
            ? <InstalledRow key={pack.id} plugin={installed} manager={manager} hardware={hardware} update={isNewerVersion(pack.version, installed.version) ? pack : undefined} />
            : <AvailableRow key={pack.id} pack={pack} manager={manager} hardware={hardware} />;
        })}
        {listReady && outsideCatalog.map((plugin) => <InstalledRow key={plugin.id} plugin={plugin} manager={manager} hardware={hardware} />)}
        {snapshot && !officialPacks && (
          <div className="flex items-center justify-between gap-4 px-3.5 py-3">
            <p role={officialError ? "alert" : "status"} className={`text-xs ${officialError ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]"}`}>
              {officialError || "正在读取官方模型列表…"}
            </p>
            {officialError && <Button size="sm" onClick={() => void manager.retryLoad()}>重试</Button>}
          </div>
        )}
        {(!snapshot || (officialPacks && empty)) && <ListStatus manager={manager} empty="还没有本地模型" />}
      </Group>
      <ManagerFooter manager={manager} />
      </InspectorBody>
    </>
  );
}

/** 插件：JavaScript 连接器，提供云端或自建服务的模型。 */
export function PluginsPanel() {
  const manager = usePluginManager(false);
  const plugins = manager.snapshot?.plugins.filter((plugin) => plugin.runtimeKind !== "model-pack") ?? [];
  return (
    <>
      <InspectorHeader title="插件" right={<PackageActions manager={manager} primary />} />
      <InspectorBody>
        {plugins.length > 0 && <Group>{plugins.map((plugin) => <InstalledRow key={plugin.id} plugin={plugin} manager={manager} />)}</Group>}
        {plugins.length === 0 && <Group><ListStatus manager={manager} empty="把 .sayit 文件拖进窗口也能安装" /></Group>}
        <ManagerFooter manager={manager} />
      </InspectorBody>
    </>
  );
}
