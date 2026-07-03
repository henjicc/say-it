import type { UnlistenFn } from "@tauri-apps/api/event";
import { CMD, EVT, cmd, on } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";
import {
  useTranscriptionStore,
  type TranscriptionEventPayload,
  type TranscriptionParams,
} from "@/store/useTranscriptionStore";
import { useUiStore } from "@/store/useUiStore";

let activeUnlisten: UnlistenFn | null = null;
let activeJobId = "";

function providerHasApiKey() {
  return !!useProviderStore.getState().profiles.find((profile) => profile.id === "funasr")?.status?.hasApiKey;
}

function normalizeParams(params: TranscriptionParams) {
  return {
    model: params.model || "fun-asr",
    vocabularyId: params.vocabularyId.trim(),
    languageHints: params.languageHints.filter(Boolean),
    diarizationEnabled: params.diarizationEnabled,
    speakerCount: params.diarizationEnabled ? params.speakerCount || null : null,
  };
}

function stopListening() {
  activeUnlisten?.();
  activeUnlisten = null;
  activeJobId = "";
}

async function listenForJob() {
  stopListening();
  activeUnlisten = await on<TranscriptionEventPayload>(EVT.transcriptionEvent, (payload) => {
    if (!payload.jobId || payload.jobId !== activeJobId) return;
    const store = useTranscriptionStore.getState();

    if (payload.stage === "uploading") {
      store.setRuntime({ stage: "uploading", statusText: "正在上传音视频文件…" });
      return;
    }
    if (payload.stage === "submitted") {
      store.setRuntime({
        stage: "recognizing",
        taskId: payload.taskId || "",
        statusText: "识别任务已提交，正在等待云端处理…",
      });
      return;
    }
    if (payload.stage === "polling") {
      store.setRuntime({
        stage: "recognizing",
        taskId: payload.taskId || store.taskId,
        statusText: `云端识别中${payload.pollCount ? `（第 ${payload.pollCount} 次查询）` : ""}…`,
      });
      return;
    }
    if (payload.stage === "completed") {
      store.setRuntime({
        stage: "completed",
        taskId: payload.taskId || store.taskId,
        statusText: "识别完成。",
        errorMessage: "",
        result: payload.result || null,
        resultView: "text",
      });
      stopListening();
      return;
    }
    if (payload.stage === "error") {
      store.setRuntime({
        stage: "error",
        statusText: payload.cancelled ? "识别已取消。" : "识别失败。",
        errorMessage: payload.message || "录音识别失败",
      });
      stopListening();
    }
  });
}

export async function startTranscription() {
  const store = useTranscriptionStore.getState();
  if (!store.selectedFile) {
    store.setRuntime({ stage: "error", statusText: "未选择文件。", errorMessage: "请先选择一个音视频文件。" });
    return;
  }

  if (useProviderStore.getState().profiles.length === 0) {
    await useProviderStore.getState().load();
  }
  if (!providerHasApiKey()) {
    store.setRuntime({
      stage: "error",
      statusText: "缺少 API Key。",
      errorMessage: "请先在设置中保存阿里云百炼 API Key。",
    });
    return;
  }

  await listenForJob();
  useTranscriptionStore.getState().setRuntime({
    stage: "uploading",
    jobId: "",
    taskId: "",
    result: null,
    errorMessage: "",
    saveMessage: "",
    statusText: "正在准备识别任务…",
  });

  try {
    const response = await cmd<{ jobId: string }>(CMD.transcriptionStart, {
      filePath: store.selectedFile.path,
      params: normalizeParams(store.params),
    });
    activeJobId = response.jobId;
    useTranscriptionStore.getState().setRuntime({
      jobId: response.jobId,
      statusText: "正在上传音视频文件…",
    });
  } catch (error) {
    stopListening();
    useTranscriptionStore.getState().setRuntime({
      stage: "error",
      statusText: "识别启动失败。",
      errorMessage: String(error || "识别启动失败"),
    });
  }
}

export async function cancelTranscription() {
  const { jobId, stage } = useTranscriptionStore.getState();
  if (!jobId || (stage !== "uploading" && stage !== "recognizing")) return;
  useTranscriptionStore.getState().setRuntime({ statusText: "正在取消识别任务…" });
  try {
    await cmd(CMD.transcriptionCancel, { jobId });
  } catch (error) {
    stopListening();
    useTranscriptionStore.getState().setRuntime({
      stage: "error",
      statusText: "取消失败。",
      errorMessage: String(error || "取消失败"),
    });
  }
}

export function openProviderSettings() {
  useUiStore.getState().setView("settings");
}

export function cleanupTranscriptionListener() {
  stopListening();
}
