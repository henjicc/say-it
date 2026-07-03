import { useEffect, useMemo, useRef } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
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
import {
  FileDropSection,
  defaultSrtName,
  useFileDrop,
  useFilePick,
} from "@/features/transcription/filePicker";
import { buildCues, cueText, formatSrtTime, plainText, toSrt } from "@/features/transcription/subtitles";
import {
  FILE_ASR_MODEL_OPTIONS,
  isSupportedFileModel,
} from "@/features/asr/modelOptions";
import { TranscriptAlignPanel } from "@/views/TranscriptAlignPanel";
import { useProviderStore } from "@/store/useProviderStore";
import {
  DEFAULT_TRANSCRIPTION_PARAMS,
  useTranscriptionStore,
  type TranscriptionParams,
  type TranscriptionTab,
} from "@/store/useTranscriptionStore";

const TABS: TabItem<TranscriptionTab>[] = [
  { key: "transcribe", label: "录音转写" },
  { key: "align", label: "文稿对齐" },
  { key: "settings", label: "通用设置" },
];

const LANGUAGE_OPTIONS = [
  { value: "zh", label: "中文" },
  { value: "en", label: "英文" },
  { value: "ja", label: "日语" },
];

function normalizeStoredParams(value: unknown): TranscriptionParams {
  const source = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const speakerCount = Number(source.speakerCount);
  return {
    ...DEFAULT_TRANSCRIPTION_PARAMS,
    model:
      typeof source.model === "string" && isSupportedFileModel(source.model)
        ? source.model
        : DEFAULT_TRANSCRIPTION_PARAMS.model,
    vocabularyId: "",
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
  const hydratedRef = useRef(false);
  const lastSavedParamsRef = useRef("");

  const funasr = providers.find((profile) => profile.id === "funasr");
  const hasApiKey = !!funasr?.status?.hasApiKey;
  const running = stage === "uploading" || stage === "recognizing";
  const { pickState, message, loadFileInfo, pickFile } = useFilePick(setSelectedFile);
  const dragActive = useFileDrop(loadFileInfo, tab === "transcribe");
  const textResult = useMemo(() => plainText(result), [result]);
  const cues = useMemo(() => buildCues(result), [result]);
  const srt = useMemo(() => toSrt(cues), [cues]);

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
              <CardDescription>选择一个本地音视频文件，上传到临时 OSS 后提交阿里云百炼录音识别。</CardDescription>
            </div>
            {selectedFile && (
              <Button size="sm" onClick={pickFile} disabled={pickState === "loading" || running}>
                重新选择
              </Button>
            )}
          </div>

          <FileDropSection
            file={selectedFile}
            dragActive={dragActive}
            disabled={pickState === "loading" || running}
            pickState={pickState}
            message={message}
            onPick={pickFile}
          />

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
      ) : tab === "align" ? (
        <TranscriptAlignPanel />
      ) : (
        <Card className="mt-2">
          <div>
            <CardTitle>通用设置</CardTitle>
            <CardDescription>录音转写与文稿对齐共用这些识别设置。</CardDescription>
          </div>

          <div className="mt-4 grid gap-4 md:grid-cols-2">
            <Field label="识别模型">
              <Select value={params.model} onChange={(event) => setParams({ model: event.target.value })}>
                {FILE_ASR_MODEL_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </Select>
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

          {saveMessage && <p className="mt-3 text-xs text-white/45">{saveMessage}</p>}
        </Card>
      )}
    </div>
  );
}
