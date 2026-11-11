import { create } from "zustand";
import {
  refreshPluginConsumers,
  type OfficialModelPack,
  type PluginSnapshot,
} from "@/features/plugins/pluginInstaller";
import { CMD, EVT, cmd, on } from "@/lib/tauri";

function packState(snapshot: PluginSnapshot | undefined, id: string) {
  return snapshot?.plugins.find((plugin) => plugin.id === id)?.modelPack?.state;
}

/** 依次装好这几个官方包；已装好的跳过，清单已装但权重没下完的续传。 */
export async function installLocalPacks(ids: readonly string[]): Promise<PluginSnapshot> {
  let snapshot = await cmd<PluginSnapshot>(CMD.listProviderPlugins);
  for (const id of ids) {
    const installed = snapshot.plugins.find((plugin) => plugin.id === id);
    if (installed?.modelPack?.state === "ready") continue;
    snapshot = installed?.modelPack
      ? await cmd<PluginSnapshot>(CMD.downloadProviderModelPack, { pluginId: id })
      : await cmd<PluginSnapshot>(CMD.installOfficialModelPack, { packId: id });
  }
  await refreshPluginConsumers();
  return snapshot;
}

interface LocalModelsState {
  snapshot?: PluginSnapshot;
  /** 官方模型目录。 */
  packs?: OfficialModelPack[];
  downloaded: Record<string, number>;
  /** 引导里正在安装的那几个包；没有在装时为空。 */
  installing: readonly string[];
  /** 读插件状态或模型目录失败。 */
  loadError: string;
  /** 最近一次安装失败，以及是哪几个包。 */
  installError?: { ids: readonly string[]; message: string };
  /** 读插件快照、模型目录并订阅进度；重复调用只刷新快照。 */
  load: () => Promise<void>;
  install: (ids: readonly string[]) => Promise<void>;
}

let subscribed = false;
let pollTimer = 0;
const POLL_MS = 2000;

/**
 * 引导里的本地模型下载状态。放在组件之外：下载期间返回上一步或关掉重开，
 * 回来仍能接上进度，也不会再发起一轮撞上后端下载租约的安装。
 * 进度事件只更新字节数；是否下完以快照为准：自己发起的看 install() 的返回，
 * 别处（插件页）发起的在下载中低频轮询。
 */
export const useLocalModels = create<LocalModelsState>((set, get) => {
  const refresh = async () => {
    const snapshot = await cmd<PluginSnapshot>(CMD.listProviderPlugins);
    set({ snapshot, loadError: "" });
    window.clearTimeout(pollTimer);
    const external = get().installing.length === 0
      && snapshot.plugins.some((plugin) => plugin.modelPack?.state === "downloading");
    if (external) pollTimer = window.setTimeout(() => void refresh().catch(() => undefined), POLL_MS);
  };
  return {
    downloaded: {},
    installing: [],
    loadError: "",
    load: async () => {
      if (!subscribed) {
        subscribed = true;
        void on<{ pluginId: string; packDownloadedBytes: number }>(EVT.modelPackProgress, (event) => {
          set((state) => ({ downloaded: { ...state.downloaded, [event.pluginId]: event.packDownloadedBytes } }));
        });
      }
      try {
        await refresh();
        if (!get().packs) set({ packs: await cmd<OfficialModelPack[]>(CMD.listOfficialModelPacks) });
      } catch (reason) {
        set({ loadError: `读取本地模型失败：${String(reason)}` });
      }
    },
    install: async (ids) => {
      if (get().installing.length) return;
      set({ installing: ids, installError: undefined });
      try {
        set({ snapshot: await installLocalPacks(ids) });
      } catch (reason) {
        set({ installError: { ids, message: `下载失败：${String(reason)}` } });
      } finally {
        set({ installing: [] });
      }
      await refresh().catch(() => undefined);
    },
  };
});

/** 这几个包合计的下载进度：已就绪的按全量计，其余按最近一次进度事件计。 */
export function useLocalModelProgress(ids: readonly string[]) {
  const { snapshot, packs, downloaded, installing, installError } = useLocalModels();
  const mine = (others: readonly string[]) => others.some((id) => ids.includes(id));
  const wanted = ids.map((id) => packs?.find((pack) => pack.id === id));
  const ready = ids.length > 0 && ids.every((id) => packState(snapshot, id) === "ready");
  const total = wanted.reduce((sum, pack) => sum + (pack?.sizeBytes ?? 0), 0);
  const done = wanted.reduce(
    (sum, pack) => sum + (pack ? (packState(snapshot, pack.id) === "ready" ? pack.sizeBytes : downloaded[pack.id] ?? 0) : 0),
    0,
  );
  return {
    ready,
    downloading: mine(installing) || ids.some((id) => packState(snapshot, id) === "downloading"),
    /** 引导里别的选项正在下载，这一个要等它下完才能开始。 */
    busyElsewhere: installing.length > 0 && !mine(installing),
    /** 目录已读到，但里面没有其中某个还没装过的包；装过清单的不需要目录也能续传。 */
    unavailable: Boolean(packs) && ids.some((id, index) => !wanted[index] && !packState(snapshot, id)),
    installError: installError && mine(installError.ids) ? installError.message : "",
    total,
    done,
  };
}
