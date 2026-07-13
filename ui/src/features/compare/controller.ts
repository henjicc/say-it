import { CMD, cmd } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useCompareStore, type CompareCellRuntime, type CompareCellStatus, type ComparePhase } from "@/store/useCompareStore";
import { ensureProviderReady } from "@/features/transcription/controller";

type RuntimeSnapshot = {
  phase: ComparePhase | "starting";
  cells: Array<{ index: number; status: CompareCellStatus; text: string; errorMessage: string }>;
  playbackProgress: { currentMs: number; durationMs: number } | null;
  error: string;
};

function applyRuntime(snapshot: RuntimeSnapshot) {
  const store = useCompareStore.getState();
  const phase = snapshot.phase === "starting" ? "finalizing" : snapshot.phase;
  const cellRuntime = store.prefs.cellModels.map<CompareCellRuntime>((_, index) => {
    const cell = snapshot.cells.find((item) => item.index === index);
    return cell || { status: "idle", text: "", errorMessage: "" };
  });
  useCompareStore.setState({
    phase,
    globalError: snapshot.error,
    playbackProgress: snapshot.playbackProgress,
    cellRuntime,
  });
}

export function applyCompareRuntime(snapshot: RuntimeSnapshot) {
  applyRuntime(snapshot);
}

export async function loadCompareRuntime() {
  applyRuntime(await cmd<RuntimeSnapshot>(CMD.getCompareRuntime));
}

export async function startCompare() {
  const store = useCompareStore.getState();
  if (store.phase !== "idle") return;
  if (!store.prefs.cellModels.some(Boolean)) {
    store.setRuntime({ globalError: "请至少选择一个模型" });
    return;
  }
  if (store.prefs.sourceMode === "upload" && !store.selectedFile) {
    store.setRuntime({ globalError: "请先选择音频文件" });
    return;
  }
  if (!(await ensureProviderReady())) {
    store.setRuntime({ globalError: "请先在设置中保存阿里云百炼 API Key" });
    return;
  }
  try {
    applyRuntime(await cmd<RuntimeSnapshot>(CMD.compareStart, {
      request: {
        sourceMode: store.prefs.sourceMode,
        filePath: store.selectedFile?.path,
        models: store.prefs.cellModels,
        deviceName: useDictPrefs.getState().prefs.micDeviceId || undefined,
        params: useDictPrefs.getState().dspParams(),
      },
    }));
  } catch (error) {
    store.setRuntime({ globalError: String(error || "启动对比失败") });
  }
}

export async function stopCompare() {
  try {
    applyRuntime(await cmd<RuntimeSnapshot>(CMD.compareStop));
  } catch (error) {
    useCompareStore.getState().setRuntime({ globalError: String(error || "停止对比失败") });
  }
}

export async function hardAbortCompare() {
  applyRuntime(await cmd<RuntimeSnapshot>(CMD.compareCancel));
}
