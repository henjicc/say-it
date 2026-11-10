import { create } from "zustand";
import {
  refreshPluginConsumers,
  type OfficialModelPack,
  type PluginSnapshot,
} from "@/features/plugins/pluginInstaller";
import { CMD, EVT, cmd, on } from "@/lib/tauri";

/** 本地离线要同时装识别模型和标点模型：Paraformer 只出无标点的文字。 */
export const LOCAL_PACK_IDS = ["official.sherpa-paraformer-online", "official.sherpa-ct-punct-zh-en"] as const;

function packState(snapshot: PluginSnapshot | undefined, id: string) {
  return snapshot?.plugins.find((plugin) => plugin.id === id)?.modelPack?.state;
}

export function localPacksReady(snapshot: PluginSnapshot | undefined): boolean {
  return LOCAL_PACK_IDS.every((id) => packState(snapshot, id) === "ready");
}

/** 依次装好两个官方包；已装好的跳过，清单已装但权重没下完的续传。 */
export async function installLocalPacks(): Promise<PluginSnapshot> {
  let snapshot = await cmd<PluginSnapshot>(CMD.listProviderPlugins);
  for (const id of LOCAL_PACK_IDS) {
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
  packs?: OfficialModelPack[];
  downloaded: Record<string, number>;
  installing: boolean;
  /** 读插件状态或模型目录失败。 */
  loadError: string;
  installError: string;
  /** 读插件快照、模型目录并订阅进度；重复调用只刷新快照。 */
  load: () => Promise<void>;
  install: () => Promise<void>;
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
    const external = !get().installing
      && LOCAL_PACK_IDS.some((id) => packState(snapshot, id) === "downloading");
    if (external) pollTimer = window.setTimeout(() => void refresh().catch(() => undefined), POLL_MS);
  };
  return {
    downloaded: {},
    installing: false,
    loadError: "",
    installError: "",
    load: async () => {
      if (!subscribed) {
        subscribed = true;
        void on<{ pluginId: string; packDownloadedBytes: number }>(EVT.modelPackProgress, (event) => {
          set((state) => ({ downloaded: { ...state.downloaded, [event.pluginId]: event.packDownloadedBytes } }));
        });
      }
      try {
        await refresh();
        if (get().packs) return;
        const packs = await cmd<OfficialModelPack[]>(CMD.listOfficialModelPacks);
        set({
          packs: LOCAL_PACK_IDS.map((id) => {
            const pack = packs.find((item) => item.id === id);
            if (!pack) throw new Error(`官方模型目录里没有 ${id}`);
            return pack;
          }),
        });
      } catch (reason) {
        set({ loadError: `读取本地模型失败：${String(reason)}` });
      }
    },
    install: async () => {
      if (get().installing) return;
      set({ installing: true, installError: "" });
      try {
        set({ snapshot: await installLocalPacks() });
      } catch (reason) {
        set({ installError: `下载失败：${String(reason)}` });
      } finally {
        set({ installing: false });
      }
      await refresh().catch(() => undefined);
    },
  };
});

/** 两个包合计的下载进度：已就绪的按全量计，其余按最近一次进度事件计。 */
export function useLocalModelProgress() {
  const { snapshot, packs, downloaded, installing } = useLocalModels();
  const total = packs?.reduce((sum, pack) => sum + pack.sizeBytes, 0) ?? 0;
  const done = (packs ?? []).reduce(
    (sum, pack) => sum + (packState(snapshot, pack.id) === "ready" ? pack.sizeBytes : downloaded[pack.id] ?? 0),
    0,
  );
  return {
    ready: localPacksReady(snapshot),
    downloading: installing || LOCAL_PACK_IDS.some((id) => packState(snapshot, id) === "downloading"),
    total,
    done,
  };
}
