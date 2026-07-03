import type { UnlistenFn } from "@tauri-apps/api/event";
import { CMD, EVT, cmd, on } from "@/lib/tauri";
import { buildOptimizedAlignCues } from "@/features/transcription/subtitles";
import { useProviderStore } from "@/store/useProviderStore";
import {
  useTranscriptionStore,
  type AlignedLine,
  type TranscriptionEventPayload,
  type TranscriptionParams,
  type TranscriptionResult,
} from "@/store/useTranscriptionStore";
import { useUiStore } from "@/store/useUiStore";

type JobTarget = "transcribe" | "align";

let activeUnlisten: UnlistenFn | null = null;
let activeJobId = "";
let activeTarget: JobTarget = "transcribe";
// 对齐流程在识别完成后才用到的上下文（本次执行捕获的文稿与缓存键）
let pendingScriptLines: string[] = [];
let pendingAlignFilePath = "";
let pendingAlignParamsKey = "";

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

export function splitScriptLines(text: string) {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function stopListening() {
  activeUnlisten?.();
  activeUnlisten = null;
  activeJobId = "";
}

async function listenForJob(target: JobTarget) {
  stopListening();
  activeTarget = target;
  activeUnlisten = await on<TranscriptionEventPayload>(EVT.transcriptionEvent, (payload) => {
    if (!payload.jobId || payload.jobId !== activeJobId) return;
    if (activeTarget === "transcribe") handleTranscribeEvent(payload);
    else handleAlignEvent(payload);
  });
}

function handleTranscribeEvent(payload: TranscriptionEventPayload) {
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
}

function handleAlignEvent(payload: TranscriptionEventPayload) {
  const store = useTranscriptionStore.getState();

  if (payload.stage === "uploading") {
    store.setRuntime({ alignStage: "uploading", alignStatusText: "正在上传音视频文件…" });
    return;
  }
  if (payload.stage === "submitted") {
    store.setRuntime({ alignStage: "recognizing", alignStatusText: "识别任务已提交，正在等待云端处理…" });
    return;
  }
  if (payload.stage === "polling") {
    store.setRuntime({
      alignStage: "recognizing",
      alignStatusText: `云端识别中${payload.pollCount ? `（第 ${payload.pollCount} 次查询）` : ""}…`,
    });
    return;
  }
  if (payload.stage === "completed") {
    stopListening();
    const result = payload.result || null;
    if (!result) {
      store.setRuntime({
        alignStage: "error",
        alignStatusText: "识别失败。",
        alignErrorMessage: "识别完成但缺少结果数据",
      });
      return;
    }
    // 缓存识别结果：同一文件 + 相同参数重复执行时只重新对齐，不重复上传识别
    store.setRuntime({
      alignRecognition: {
        filePath: pendingAlignFilePath,
        paramsKey: pendingAlignParamsKey,
        result,
      },
    });
    void runAlign(result, pendingScriptLines);
    return;
  }
  if (payload.stage === "error") {
    store.setRuntime({
      alignStage: "error",
      alignStatusText: payload.cancelled ? "已取消。" : "识别失败。",
      alignErrorMessage: payload.message || "录音识别失败",
    });
    stopListening();
  }
}

function flattenWords(result: TranscriptionResult) {
  const transcript = result.transcripts?.[0];
  return (transcript?.sentences || [])
    .flatMap((sentence) => sentence.words || [])
    .map((word) => ({
      beginTime: Math.max(0, Math.round(Number(word.beginTime) || 0)),
      endTime: Math.max(0, Math.round(Number(word.endTime) || 0)),
      text: word.text || "",
      // 后端对齐忽略该字段；保留它是为了替换段的识别字幕带标点
      punctuation: word.punctuation ?? null,
    }));
}

async function runAlign(result: TranscriptionResult, scriptLines: string[]) {
  useTranscriptionStore.getState().setRuntime({ alignStage: "aligning", alignStatusText: "正在对齐文稿…" });
  try {
    const words = flattenWords(result);
    const lines = await cmd<AlignedLine[]>(CMD.alignTranscript, { words, scriptLines });
    useTranscriptionStore.getState().setRuntime({
      alignStage: "completed",
      alignedLines: lines,
      alignOptimizedCues: buildOptimizedAlignCues(lines, words),
      alignStatusText: "对齐完成。",
      alignErrorMessage: "",
    });
  } catch (error) {
    useTranscriptionStore.getState().setRuntime({
      alignStage: "error",
      alignStatusText: "对齐失败。",
      alignErrorMessage: String(error || "文稿对齐失败"),
    });
  }
}

async function ensureProviderReady() {
  if (useProviderStore.getState().profiles.length === 0) {
    await useProviderStore.getState().load();
  }
  return providerHasApiKey();
}

export async function startTranscription() {
  const store = useTranscriptionStore.getState();
  if (!store.selectedFile) {
    store.setRuntime({ stage: "error", statusText: "未选择文件。", errorMessage: "请先选择一个音视频文件。" });
    return;
  }
  if (store.alignStage === "uploading" || store.alignStage === "recognizing" || store.alignStage === "aligning") {
    store.setRuntime({
      stage: "error",
      statusText: "任务冲突。",
      errorMessage: "文稿对齐正在进行中，请等待完成或先取消。",
    });
    return;
  }

  if (!(await ensureProviderReady())) {
    store.setRuntime({
      stage: "error",
      statusText: "缺少 API Key。",
      errorMessage: "请先在设置中保存阿里云百炼 API Key。",
    });
    return;
  }

  await listenForJob("transcribe");
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

export async function startAlignment() {
  const store = useTranscriptionStore.getState();
  const file = store.alignFile;
  const scriptLines = splitScriptLines(store.scriptText);
  if (!file) {
    store.setRuntime({ alignStage: "error", alignStatusText: "未选择文件。", alignErrorMessage: "请先选择一个音视频文件。" });
    return;
  }
  if (scriptLines.length === 0) {
    store.setRuntime({ alignStage: "error", alignStatusText: "缺少文稿。", alignErrorMessage: "请先输入一行一句的文稿。" });
    return;
  }
  if (store.stage === "uploading" || store.stage === "recognizing") {
    store.setRuntime({
      alignStage: "error",
      alignStatusText: "任务冲突。",
      alignErrorMessage: "录音转写正在进行中，请等待完成或先取消。",
    });
    return;
  }

  if (!(await ensureProviderReady())) {
    store.setRuntime({
      alignStage: "error",
      alignStatusText: "缺少 API Key。",
      alignErrorMessage: "请先在设置中保存阿里云百炼 API Key。",
    });
    return;
  }

  const paramsKey = JSON.stringify(normalizeParams(store.params));
  pendingScriptLines = scriptLines;
  pendingAlignFilePath = file.path;
  pendingAlignParamsKey = paramsKey;

  const cache = store.alignRecognition;
  if (cache && cache.filePath === file.path && cache.paramsKey === paramsKey) {
    store.setRuntime({
      alignedLines: null,
      alignOptimizedCues: null,
      alignErrorMessage: "",
      alignSaveMessage: "",
      alignStatusText: "复用上次识别结果…",
    });
    await runAlign(cache.result, scriptLines);
    return;
  }

  await listenForJob("align");
  useTranscriptionStore.getState().setRuntime({
    alignStage: "uploading",
    alignJobId: "",
    alignedLines: null,
    alignOptimizedCues: null,
    alignErrorMessage: "",
    alignSaveMessage: "",
    alignStatusText: "正在准备识别任务…",
  });

  try {
    const response = await cmd<{ jobId: string }>(CMD.transcriptionStart, {
      filePath: file.path,
      params: normalizeParams(store.params),
    });
    activeJobId = response.jobId;
    useTranscriptionStore.getState().setRuntime({
      alignJobId: response.jobId,
      alignStatusText: "正在上传音视频文件…",
    });
  } catch (error) {
    stopListening();
    useTranscriptionStore.getState().setRuntime({
      alignStage: "error",
      alignStatusText: "识别启动失败。",
      alignErrorMessage: String(error || "识别启动失败"),
    });
  }
}

export async function cancelAlignment() {
  const { alignJobId, alignStage } = useTranscriptionStore.getState();
  if (!alignJobId || (alignStage !== "uploading" && alignStage !== "recognizing")) return;
  useTranscriptionStore.getState().setRuntime({ alignStatusText: "正在取消识别任务…" });
  try {
    await cmd(CMD.transcriptionCancel, { jobId: alignJobId });
  } catch (error) {
    stopListening();
    useTranscriptionStore.getState().setRuntime({
      alignStage: "error",
      alignStatusText: "取消失败。",
      alignErrorMessage: String(error || "取消失败"),
    });
  }
}

export function openProviderSettings() {
  useUiStore.getState().setView("settings");
}

export function cleanupTranscriptionListener() {
  stopListening();
}
