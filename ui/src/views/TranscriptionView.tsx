import { useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
import { Collapse } from "@/components/ui/Collapse";
import { CheckField, Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import {
  cancelTranscription,
  openProviderSettings,
  startTranscription,
} from "@/features/transcription/controller";
import { buildCues, cueText, formatSrtTime, plainText, toSrt } from "@/features/transcription/subtitles";
import { useProviderStore } from "@/store/useProviderStore";
import {
  DEFAULT_TRANSCRIPTION_PARAMS,
  useTranscriptionStore,
  type SelectedTranscriptionFile,
  type TranscriptionParams,
  type TranscriptionTab,
} from "@/store/useTranscriptionStore";

const MAX_FILE_SIZE = 2 * 1024 * 1024 * 1024;
const SUPPORTED_EXTENSIONS = [
  "aac",
  "amr",
  "avi",
  "flac",
  "flv",
  "m4a",
  "mkv",
  "mov",
  "mp3",
  "mp4",
  "mpeg",
  "ogg",
  "opus",
  "wav",
  "webm",
  "wma",
  "wmv",
];

const TABS: TabItem<TranscriptionTab>[] = [
  { key: "transcribe", label: "录音转写" },
  { key: "align", label: "文稿对齐" },
];

const MODEL_OPTIONS = [
  { value: "fun-asr", label: "fun-asr（推荐）" },
  { value: "fun-asr-2025-11-07", label: "fun-asr-2025-11-07" },
  { value: "fun-asr-2025-08-25", label: "fun-asr-2025-08-25" },
  { value: "fun-asr-mtl", label: "fun-asr-mtl" },
  { value: "fun-asr-mtl-2025-08-25", label: "fun-asr-mtl-2025-08-25" },
];

const LANGUAGE_OPTIONS = [
  { value: "zh", label: "中文" },
  { value: "en", label: "英文" },
  { value: "ja", label: "日语" },
];

type PickState = "idle" | "loading" | "error";

function formatSize(size: number) {
  if (size >= 1024 * 1024 * 1024) return `${(size / 1024 / 1024 / 1024).toFixed(2)} GB`;
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${size} B`;
}

function extensionOf(name: string) {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function validateFile(file: SelectedTranscriptionFile) {
  const extension = extensionOf(file.name || file.path);
  if (file.size > MAX_FILE_SIZE) return "文件超过 2GB，Fun-ASR 录音文件识别可能无法处理。";
  if (!SUPPORTED_EXTENSIONS.includes(extension)) {
    return "文件扩展名不在 Fun-ASR 官方支持列表内，仍可尝试提交，以服务端结果为准。";
  }
  return "";
}

function normalizeStoredParams(value: unknown): TranscriptionParams {
  const source = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const speakerCount = Number(source.speakerCount);
  return {
    ...DEFAULT_TRANSCRIPTION_PARAMS,
    model: typeof source.model === "string" && source.model ? source.model : DEFAULT_TRANSCRIPTION_PARAMS.model,
    vocabularyId: typeof source.vocabularyId === "string" ? source.vocabularyId : "",
    languageHints: Array.isArray(source.languageHints) ? source.languageHints.filter((item): item is string => typeof item === "string") : [],
    diarizationEnabled: !!source.diarizationEnabled,
    speakerCount: Number.isFinite(speakerCount) && speakerCount > 0 ? speakerCount : null,
  };
}

function sameParams(a: TranscriptionParams, b: TranscriptionParams) {
  return JSON.stringify(a) === JSON.stringify(b);
}

async function copyText(text: string) {
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    document.execCommand("copy");
    textarea.remove();
  }
}

function defaultSrtName(file: SelectedTranscriptionFile | null) {
  const name = file?.name || "录音识别结果";
  const dot = name.lastIndexOf(".");
  return `${dot > 0 ? name.slice(0, dot) : name}.srt`;
}

export function TranscriptionView() {
  const {
    tab,
    selectedFile,
    params,
    stage,
    statusText,
    errorMessage,
    result,
    resultView,
    saveMessage,
    setTab,
    setSelectedFile,
    setParams,
    replaceParams,
    setRuntime,
  } = useTranscriptionStore();
  const providers = useProviderStore((s) => s.profiles);
  const loadProviders = useProviderStore((s) => s.load);
  const updateProviderConfig = useProviderStore((s) => s.updateConfig);
  const [pickState, setPickState] = useState<PickState>("idle");
  const [message, setMessage] = useState("");
  const [dragActive, setDragActive] = useState(false);
  const hydratedRef = useRef(false);
  const lastSavedParamsRef = useRef("");

  const funasr = providers.find((profile) => profile.id === "funasr");
  const hasApiKey = !!funasr?.status?.hasApiKey;
  const running = stage === "uploading" || stage === "recognizing";
  const validationMessage = useMemo(
    () => (selectedFile ? validateFile(selectedFile) : ""),
    [selectedFile],
  );
  const textResult = useMemo(() => plainText(result), [result]);
  const cues = useMemo(() => buildCues(result), [result]);
  const srt = useMemo(() => toSrt(cues), [cues]);

  const loadFileInfo = async (path: string) => {
    setPickState("loading");
    setMessage("");
    try {
      const file = await cmd<SelectedTranscriptionFile>(CMD.getLocalFileInfo, { filePath: path });
      setSelectedFile(file);
      setMessage("");
      setPickState("idle");
    } catch (err) {
      setPickState("error");
      setMessage(err instanceof Error ? err.message : String(err || "读取文件信息失败"));
    }
  };

  const pickFile = async () => {
    setPickState("loading");
    setMessage("");
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "音视频文件", extensions: SUPPORTED_EXTENSIONS }],
      });
      if (typeof selected !== "string") {
        setPickState("idle");
        return;
      }
      await loadFileInfo(selected);
    } catch (err) {
      setPickState("error");
      setMessage(err instanceof Error ? err.message : String(err || "选择文件失败"));
    }
  };

  const toggleLanguageHint = (value: string) => {
    const next = params.languageHints.includes(value)
      ? params.languageHints.filter((item) => item !== value)
      : [...params.languageHints, value];
    setParams({ languageHints: next });
  };

  const exportSrt = async () => {
    if (cues.length === 0) {
      setRuntime({ saveMessage: "当前没有可导出的字幕。" });
      return;
    }
    try {
      const path = await save({
        defaultPath: defaultSrtName(selectedFile),
        filters: [{ name: "SRT 字幕", extensions: ["srt"] }],
      });
      if (!path) return;
      await cmd(CMD.saveTextFile, { path, content: srt });
      setRuntime({ saveMessage: `已导出：${path}` });
    } catch (error) {
      setRuntime({ saveMessage: `导出失败：${String(error)}` });
    }
  };

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    const stored = normalizeStoredParams(funasr?.config?.transcription);
    const key = JSON.stringify(stored);
    lastSavedParamsRef.current = key;
    hydratedRef.current = true;
    if (!sameParams(params, stored)) replaceParams(stored);
  }, [funasr?.config?.transcription]);

  useEffect(() => {
    if (!hydratedRef.current) return;
    const key = JSON.stringify(params);
    if (key === lastSavedParamsRef.current) return;
    const timer = window.setTimeout(async () => {
      try {
        await updateProviderConfig("funasr", { transcription: params });
        lastSavedParamsRef.current = key;
        setRuntime({ saveMessage: "识别参数已保存。" });
      } catch (error) {
        setRuntime({ saveMessage: `识别参数保存失败：${String(error)}` });
      }
    }, 450);
    return () => window.clearTimeout(timer);
  }, [params, updateProviderConfig, setRuntime]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "over") {
          setDragActive(true);
          return;
        }
        if (payload.type === "leave") {
          setDragActive(false);
          return;
        }
        if (payload.type === "drop") {
          setDragActive(false);
          const firstPath = payload.paths[0];
          if (firstPath) void loadFileInfo(firstPath);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <div className="flex flex-col gap-4 py-2">
      <div>
        <h1 className="text-xl font-semibold text-white">录音识别</h1>
        <p className="mt-1 text-sm text-white/45">处理本地音视频文件，生成转写文本或用于文稿对齐的时间轴。</p>
      </div>

      <Tabs<TranscriptionTab> tabs={TABS} active={tab} onChange={setTab} />

      {tab === "transcribe" ? (
        <Card className="mt-2">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <CardTitle>录音转写</CardTitle>
              <CardDescription>选择一个本地音视频文件，上传到临时 OSS 后提交 Fun-ASR 录音文件识别。</CardDescription>
            </div>
            {selectedFile && (
              <Button size="sm" onClick={pickFile} disabled={pickState === "loading" || running}>
                重新选择
              </Button>
            )}
          </div>

          <button
            type="button"
            onClick={pickFile}
            disabled={pickState === "loading" || running}
            className={cn(
              "mt-5 flex min-h-44 w-full flex-col items-center justify-center rounded-2xl border border-dashed px-6 py-8 text-center transition-colors",
              "focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--color-accent)_55%,transparent)]",
              dragActive
                ? "border-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_16%,transparent)]"
                : "border-white/16 bg-white/[0.035] hover:border-[color-mix(in_srgb,var(--color-accent)_42%,transparent)] hover:bg-white/[0.055]",
              (pickState === "loading" || running) && "cursor-wait opacity-75",
            )}
          >
            <span className="flex h-11 w-11 items-center justify-center rounded-full border border-white/12 bg-white/[0.06] text-white/70">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="h-5 w-5" aria-hidden>
                <path d="M12 16V4" />
                <path d="m7 9 5-5 5 5" />
                <path d="M5 18.5h14" />
              </svg>
            </span>
            <span className="mt-4 text-base font-medium text-white">
              {pickState === "loading" ? "正在读取文件信息…" : selectedFile ? selectedFile.name : "选择或拖放音视频文件"}
            </span>
            <span className="mt-2 max-w-xl text-sm leading-relaxed text-white/45">
              支持 mp3、wav、m4a、mp4、flac、ogg、webm 等常见格式，单文件最大 2GB。
            </span>
          </button>

          {selectedFile && (
            <div className="mt-4 grid gap-3 rounded-xl border border-white/10 bg-white/[0.035] p-4 text-sm md:grid-cols-[1fr_auto]">
              <div className="min-w-0">
                <p className="truncate font-medium text-white">{selectedFile.name}</p>
                <p className="mt-1 truncate text-white/42">{selectedFile.path}</p>
              </div>
              <div className="flex items-center gap-2 text-white/55 md:justify-end">
                <span>{formatSize(selectedFile.size)}</span>
                <span className="h-1 w-1 rounded-full bg-white/28" aria-hidden />
                <span>{extensionOf(selectedFile.name || selectedFile.path).toUpperCase() || "未知格式"}</span>
              </div>
            </div>
          )}

          {(validationMessage || message) && (
            <p className={cn("mt-3 text-sm", pickState === "error" ? "text-[#ff8589]" : "text-[#f5c56f]")}>
              {message || validationMessage}
            </p>
          )}

          <div className="mt-4 flex flex-wrap items-center gap-3">
            <Button variant="primary" onClick={startTranscription} disabled={!selectedFile || running}>
              开始识别
            </Button>
            {running && (
              <Button variant="danger" onClick={cancelTranscription}>
                取消
              </Button>
            )}
            {!hasApiKey && (
              <Button onClick={openProviderSettings}>
                去设置 API Key
              </Button>
            )}
          </div>

          <div className="mt-4 rounded-xl border border-white/10 bg-white/[0.03] p-4">
            <div className="flex items-center gap-3">
              <span
                className={cn(
                  "h-2.5 w-2.5 rounded-full",
                  stage === "completed" && "bg-[#25c36f]",
                  stage === "error" && "bg-[#ff6b6b]",
                  running && "animate-pulse bg-[var(--color-accent)]",
                  stage === "idle" && "bg-white/28",
                )}
                aria-hidden
              />
              <p className="text-sm text-white/70">{statusText || "等待选择文件。"}</p>
            </div>
            {errorMessage && <p className="mt-2 text-sm text-[#ff8589]">{errorMessage}</p>}
            {saveMessage && <p className="mt-2 text-xs text-white/45">{saveMessage}</p>}
          </div>

          <Collapse title="识别参数" subtitle="默认配置适合大多数录音" className="mt-4">
            <div className="grid gap-4 md:grid-cols-2">
              <Field label="模型版本">
                <Select value={params.model} onChange={(event) => setParams({ model: event.target.value })}>
                  {MODEL_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="热词 ID" hint="实时识别热词不自动复用到录音识别；需要时手动填写 fun-asr 词表 ID。">
                <Input
                  value={params.vocabularyId}
                  onChange={(event) => setParams({ vocabularyId: event.target.value })}
                  placeholder="留空"
                />
              </Field>
            </div>

            <div className="mt-4">
              <p className="text-xs font-medium text-white/60">语种提示</p>
              <div className="mt-2 flex flex-wrap gap-4">
                <CheckField checked={params.languageHints.length === 0} onChange={() => setParams({ languageHints: [] })}>
                  自动
                </CheckField>
                {LANGUAGE_OPTIONS.map((lang) => (
                  <CheckField key={lang.value} checked={params.languageHints.includes(lang.value)} onChange={() => toggleLanguageHint(lang.value)}>
                    {lang.label}
                  </CheckField>
                ))}
              </div>
            </div>

            <div className="mt-4 grid gap-4 md:grid-cols-2">
              <CheckField checked={params.diarizationEnabled} onChange={(checked) => setParams({ diarizationEnabled: checked })}>
                说话人分离
              </CheckField>
              <Field label="说话人数" hint="留空自动判断；开启说话人分离后可填 2 到 100。">
                <Input
                  type="number"
                  min={2}
                  max={100}
                  disabled={!params.diarizationEnabled}
                  value={params.speakerCount ?? ""}
                  onChange={(event) => {
                    const value = Number(event.target.value);
                    setParams({ speakerCount: Number.isFinite(value) && value > 0 ? value : null });
                  }}
                  placeholder="自动"
                />
              </Field>
            </div>
          </Collapse>

          {result && (
            <div className="mt-5 border-t border-white/10 pt-5">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <Tabs
                  tabs={[
                    { key: "text", label: "纯文本" },
                    { key: "subtitles", label: "字幕" },
                  ]}
                  active={resultView}
                  onChange={(value) => setRuntime({ resultView: value })}
                />
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" onClick={() => copyText(textResult)}>
                    复制文本
                  </Button>
                  <Button size="sm" onClick={exportSrt} disabled={cues.length === 0}>
                    导出 SRT
                  </Button>
                </div>
              </div>

              {resultView === "text" ? (
                <textarea
                  readOnly
                  value={textResult}
                  className="mt-4 min-h-72 w-full resize-y rounded-xl border border-white/10 bg-white/[0.035] px-4 py-3 text-sm leading-7 text-white/82 outline-none"
                />
              ) : (
                <div className="mt-4 max-h-[34rem] overflow-auto rounded-xl border border-white/10 bg-white/[0.035]">
                  {cues.length === 0 ? (
                    <p className="p-4 text-sm text-white/45">当前结果没有可展示的句级时间戳。</p>
                  ) : (
                    cues.map((cue) => (
                      <div key={cue.index} className="grid gap-2 border-b border-white/8 px-4 py-3 last:border-b-0 md:grid-cols-[3rem_15rem_1fr]">
                        <span className="text-xs tabular-nums text-white/35">{cue.index}</span>
                        <span className="font-mono text-xs text-white/50">
                          {formatSrtTime(cue.beginMs)} → {formatSrtTime(cue.endMs)}
                        </span>
                        <span className="text-sm leading-6 text-white/82">{cueText(cue)}</span>
                      </div>
                    ))
                  )}
                </div>
              )}
            </div>
          )}
        </Card>
      ) : (
        <Card className="mt-2">
          <CardTitle>文稿对齐</CardTitle>
          <CardDescription>后续任务会在这里接入逐行文稿与词级时间戳对齐。</CardDescription>
        </Card>
      )}
    </div>
  );
}
