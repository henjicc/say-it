import { providerReady } from "@/features/models/providerModels";
import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { CheckField } from "@/components/ui/Field";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Input } from "@/components/ui/Input";
import { SectionHeader } from "@/components/ui/SectionHeader";
import { Switch } from "@/components/ui/Switch";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { SubtitleEditor } from "@/components/transcription/SubtitleEditor";
import { copyText } from "@/lib/clipboard";
import { CMD, cmd, type TranscriptSummary } from "@/lib/tauri";
import {
  cancelTranscription,
  openProviderSettings,
  startTranscription,
} from "@/features/transcription/controller";
import {
  FileCard,
  type FileCardStatusTone,
  defaultSrtName,
  useFileDrop,
  useFilePick,
} from "@/features/transcription/filePicker";
import { editablePlainText, plainText, speakerText } from "@/features/transcription/subtitles";
import { normalizeStoredParams, parseSpeakerCount, useTranscriptionParamsSync } from "@/features/transcription/paramsSync";
import { FILE_ASR_MODEL_OPTIONS, supportsAlignmentTimestamps } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ModelPicker } from "@/features/models/ModelPicker";
import { useAsrUsageIssue, useInUsagePage, useOfferedAsrOptions } from "@/features/models/usage";
import { MeetingPanel } from "@/views/transcription/MeetingPanel";
import { useProviderStore } from "@/store/useProviderStore";
import {
  useTranscriptionStore,
  type TranscriptionParams,
  type TranscriptionResult,
} from "@/store/useTranscriptionStore";

const LANGUAGE_OPTIONS = [
  { value: "zh", label: "中文" },
  { value: "en", label: "英文" },
  { value: "ja", label: "日语" },
];

const TRANSCRIPTION_SCENES = ["transcription"];

/** 转写识别模型。参数存在默认识别供应商的配置里；在「用途」页修改时转写页未打开，直接写回那里。label 是「用途」页里按功能名显示的行名。 */
export function TranscriptionModelRow({ label = "识别模型" }: { label?: string } = {}) {
  useModelCatalogRevision();
  const params = useTranscriptionStore((s) => s.params);
  const setParams = useTranscriptionStore((s) => s.setParams);
  const asrProvider = useProviderStore((s) => s.profiles.find((profile) => profile.id === s.effective("asr")));
  const updateProviderConfig = useProviderStore((s) => s.updateConfig);
  const inUsagePage = useInUsagePage();
  const [error, setError] = useState("");
  const stored = normalizeStoredParams(asrProvider?.config?.transcription);
  const model = inUsagePage ? stored.model : params.model;
  const options = useOfferedAsrOptions(FILE_ASR_MODEL_OPTIONS, model);
  const issue = useAsrUsageIssue(model, TRANSCRIPTION_SCENES);
  const change = (value: string) => {
    // 在这里选和在「用途」里选是同一项设置，两边都写回默认值。
    setParams({ model: value });
    if (!asrProvider) {
      setError("没有可用的识别供应商，暂时无法保存。");
      return;
    }
    setError("");
    updateProviderConfig(asrProvider.id, { transcription: { ...stored, model: value } })
      .catch((reason: unknown) => setError(`保存失败：${String(reason)}`));
  };
  return (
    <Row
      label={label}
      controlId="transcription-asr-model"
      hint="识别结果带时间戳，同时用于文稿对齐。"
      message={error
        ? <span role="alert" className="text-[var(--color-err)]">{error}</span>
        : issue ?? (model && !supportsAlignmentTimestamps(model) && (
          <span role="status">这个模型没有逐字时间，字幕整段一条、不能做文稿对齐，只适合转出文字。</span>
        ))}
    >
      <div className={ROW_CONTROL_W}>
        <ModelPicker
          id="transcription-asr-model"
          value={model}
          options={options}
          panelLabel="选择语音识别模型"
          onChange={change}
        />
      </div>
    </Row>
  );
}

/**
 * 识别设置的读取与回写：参数存在默认识别供应商的配置里。用到识别参数的页面（转写、生成字幕）各自调用。
 */
export function useTranscriptionSettings() {
  const params = useTranscriptionStore((s) => s.params);
  const replaceParams = useTranscriptionStore((s) => s.replaceParams);
  const setRuntime = useTranscriptionStore((s) => s.setRuntime);
  const providers = useProviderStore((s) => s.profiles);
  const effectiveAsrId = useProviderStore((s) => s.effective("asr"));
  const loadProviders = useProviderStore((s) => s.load);
  const updateProviderConfig = useProviderStore((s) => s.updateConfig);
  const asrProvider = providers.find((profile) => profile.id === effectiveAsrId);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  const saveParams = useCallback(
    async (next: TranscriptionParams) => {
      if (!asrProvider) return;
      await updateProviderConfig(asrProvider.id, { transcription: next });
    },
    [asrProvider, updateProviderConfig],
  );
  const reportSave = useCallback(
    (saveMessage: string) => setRuntime({ saveMessage }),
    [setRuntime],
  );

  useTranscriptionParamsSync({
    stored: asrProvider?.config?.transcription,
    params,
    enabled: !!asrProvider,
    replaceParams,
    save: saveParams,
    onMessage: reportSave,
  });

  return { hasApiKey: Boolean(asrProvider && providerReady(asrProvider, "asr")) };
}

/**
 * 选文件并识别。转写出文字稿，生成字幕出可编辑的字幕；两处共用同一个文件与识别结果，
 * 在一处识别过，另一处直接看到结果。
 */
export function FileRecognitionPanel({ output }: { output: "text" | "subtitles" }) {
  useModelCatalogRevision();
  const {
    selectedFile,
    stage,
    statusText,
    errorMessage,
    result,
    editorCues,
    saveMessage,
    summary,
    jobId,
    setSelectedFile,
    setRuntime,
  } = useTranscriptionStore();
  const { hasApiKey } = useTranscriptionSettings();
  const running = stage === "uploading" || stage === "recognizing";
  const { pickState, message, loadFileInfo, pickFile } = useFilePick(setSelectedFile);
  const dragActive = useFileDrop(loadFileInfo);
  const textResult = useMemo(() => plainText(result), [result]);

  const exportSrt = async () => {
    if (!editorCues || editorCues.length === 0) {
      setRuntime({ saveMessage: "当前没有可导出的字幕。" });
      return;
    }
    try {
      const path = await save({
        defaultPath: defaultSrtName(selectedFile),
        filters: [{ name: "SRT 字幕", extensions: ["srt"] }],
      });
      if (!path) return;
      await cmd(CMD.saveSubtitleSrt, { path, cues: editorCues });
      setRuntime({ saveMessage: `已导出：${path}` });
    } catch (error) {
      setRuntime({ saveMessage: `导出失败：${String(error)}` });
    }
  };

  const statusTone: FileCardStatusTone =
    stage === "completed" ? "ok" : stage === "error" ? "err" : running ? "running" : "idle";
  const cardStatusText =
    statusText || (selectedFile ? "已就绪，可以开始识别。" : "等待选择文件。");

  return (
    <>
      {output === "subtitles" && <Group><TranscriptionModelRow /></Group>}
      <FileCard
        file={selectedFile}
        dragActive={dragActive}
        disabled={pickState === "loading" || running}
        pickState={pickState}
        message={message}
        onPick={pickFile}
        statusTone={statusTone}
        statusText={cardStatusText}
        errorMessage={errorMessage}
        hint={stage !== "completed" ? saveMessage : undefined}
        actions={
          <>
            <Button size="sm" onClick={pickFile} disabled={pickState === "loading" || running}>
              重新选择
            </Button>
            {!hasApiKey && (
              <Button size="sm" onClick={openProviderSettings}>
                去设置 API Key
              </Button>
            )}
            {running ? (
              <Button size="sm" variant="danger" onClick={cancelTranscription}>
                取消
              </Button>
            ) : (
              <Button size="sm" variant="primary" onClick={startTranscription} disabled={!selectedFile}>
                开始识别
              </Button>
            )}
          </>
        }
      />

      {result && output === "text" && (
        <section className="flex flex-col gap-2">
          <SectionHeader title="识别结果" />
          <Group>
            <textarea
              readOnly
              aria-label="识别文本"
              value={textResult}
              className="block min-h-72 w-full resize-y bg-transparent px-3.5 py-3 text-sm leading-7 text-[var(--color-fg-muted)] outline-none"
            />
            <div className="flex min-h-[var(--row-h)] items-center justify-between gap-3 px-3.5">
              <span className="min-w-0 truncate text-xs text-[var(--color-fg-subtle)]" role="status">{saveMessage}</span>
              <Button size="sm" onClick={() => copyText(textResult)}>
                复制文本
              </Button>
            </div>
          </Group>
        </section>
      )}
      {result && output === "subtitles" && (
        <section className="flex flex-col gap-2">
          <SectionHeader title="字幕" />
          <SubtitleEditor
            mediaPath={selectedFile?.path ?? null}
            cues={editorCues ?? []}
            onCuesChange={(next) => setRuntime({ editorCues: next })}
            footer={
              <>
                <Button size="sm" onClick={() => copyText(editablePlainText(editorCues ?? []))}>
                  复制文本
                </Button>
                <Button size="sm" variant="primary" onClick={exportSrt} disabled={!editorCues || editorCues.length === 0}>
                  导出 SRT
                </Button>
              </>
            }
          />
          {saveMessage && <p className="px-0.5 text-xs text-[var(--color-fg-subtle)]" role="status">{saveMessage}</p>}
        </section>
      )}
      {result && output === "text" && jobId && <SummarySection key={jobId} jobId={jobId} result={result} summary={summary} />}
    </>
  );
}

/** 转写页内容区；当前显示哪一部分由转写 store 的 tab 决定。 */
export function TranscriptionView() {
  const { tab, params, saveMessage, setParams } = useTranscriptionStore();
  useTranscriptionSettings();

  const toggleLanguageHint = (value: string) => {
    const next = params.languageHints.includes(value)
      ? params.languageHints.filter((item) => item !== value)
      : [...params.languageHints, value];
    setParams({ languageHints: next });
  };

  return (
    <InspectorBody>
      {tab === "transcribe" ? (
        <FileRecognitionPanel output="text" />
      ) : tab === "meeting" ? (
        <MeetingPanel />
      ) : (
        <>
          <Group>
            <TranscriptionModelRow />
            <Row label="语种提示">
              <div className="flex flex-wrap justify-end gap-x-4 gap-y-2">
                <CheckField checked={params.languageHints.length === 0} onChange={() => setParams({ languageHints: [] })}>
                  自动
                </CheckField>
                {LANGUAGE_OPTIONS.map((lang) => (
                  <CheckField key={lang.value} checked={params.languageHints.includes(lang.value)} onChange={() => toggleLanguageHint(lang.value)}>
                    {lang.label}
                  </CheckField>
                ))}
              </div>
            </Row>
          </Group>

          <Group title="说话人">
            <Row label="说话人分离">
              <Switch checked={params.diarizationEnabled} onChange={(checked) => setParams({ diarizationEnabled: checked })} label="说话人分离" />
            </Row>
            <Row label="说话人数" controlId="transcription-speaker-count" hint="留空自动判断；开启说话人分离后可填 2 到 100。" disabled={!params.diarizationEnabled}>
              <Input
                id="transcription-speaker-count"
                className="w-32"
                type="number"
                min={2}
                max={100}
                disabled={!params.diarizationEnabled}
                value={params.speakerCount ?? ""}
                onChange={(event) => setParams({ speakerCount: parseSpeakerCount(event.target.value) })}
                placeholder="自动"
              />
            </Row>
          </Group>

          {saveMessage && <p className="px-0.5 text-xs text-[var(--color-fg-subtle)]" role="status">{saveMessage}</p>}
        </>
      )}
    </InspectorBody>
  );
}

/** 用默认大模型把带说话人的识别结果整理成会议纪要；生成在后端进行，结果也存在后端。 */
function SummarySection({
  jobId,
  result,
  summary,
}: {
  jobId: string;
  result: TranscriptionResult;
  summary: TranscriptSummary | null;
}) {
  const [error, setError] = useState("");
  // 纪要属于生成它的识别任务，换了识别结果就不再显示。
  const mine = summary?.jobId === jobId ? summary : null;
  const running = mine?.running ?? false;
  const generate = () => {
    setError("");
    cmd(CMD.summarizeTranscript, { jobId, text: speakerText(result) }).catch((reason: unknown) => setError(String(reason)));
  };
  const shownError = error || mine?.error || "";
  return (
    <section className="flex flex-col gap-2">
      <SectionHeader
        title="纪要"
        right={<Button size="sm" variant="text" disabled={running} onClick={generate}>{running ? "正在生成…" : mine?.text ? "重新生成" : "生成纪要"}</Button>}
      />
      {shownError && <p role="alert" className="px-0.5 text-xs text-[var(--color-err)]">{shownError}</p>}
      {mine?.text && (
        <Group>
          <textarea
            readOnly
            aria-label="会议纪要"
            value={mine.text}
            className="block min-h-48 w-full resize-y bg-transparent px-3.5 py-3 text-sm leading-7 text-[var(--color-fg-muted)] outline-none"
          />
          <div className="flex min-h-[var(--row-h)] items-center justify-end px-3.5">
            <Button size="sm" onClick={() => copyText(mine.text)}>复制纪要</Button>
          </div>
        </Group>
      )}
    </section>
  );
}
