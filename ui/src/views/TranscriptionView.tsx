import { providerReady } from "@/features/models/providerModels";
import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { CheckField } from "@/components/ui/Field";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Input } from "@/components/ui/Input";
import { SectionHeader } from "@/components/ui/SectionHeader";
import { Switch } from "@/components/ui/Switch";
import { Tabs } from "@/components/ui/Tabs";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { SubtitleEditor } from "@/components/transcription/SubtitleEditor";
import { copyText } from "@/lib/clipboard";
import { CMD, cmd } from "@/lib/tauri";
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
import { editablePlainText, plainText } from "@/features/transcription/subtitles";
import { normalizeStoredParams, parseSpeakerCount, useTranscriptionParamsSync } from "@/features/transcription/paramsSync";
import { FILE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ModelPicker } from "@/features/models/ModelPicker";
import { UsageLinkRow, useAsrUsageIssue, useInUsagePage, useOfferedAsrOptions, useUsageEditable } from "@/features/models/usage";
import { TranscriptAlignPanel } from "@/views/TranscriptAlignPanel";
import { useProviderStore } from "@/store/useProviderStore";
import {
  useTranscriptionStore,
  type TranscriptionParams,
  type TranscriptionTab,
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
  const editable = useUsageEditable();
  const [error, setError] = useState("");
  const stored = normalizeStoredParams(asrProvider?.config?.transcription);
  const model = inUsagePage ? stored.model : params.model;
  const options = useOfferedAsrOptions(FILE_ASR_MODEL_OPTIONS, model);
  const issue = useAsrUsageIssue(model, TRANSCRIPTION_SCENES);
  if (!editable) {
    return <UsageLinkRow label={label} value={options.find((option) => option.value === model)?.label ?? model} message={issue} />;
  }
  const change = (value: string) => {
    setParams({ model: value });
    if (!inUsagePage) return;
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
      message={error ? <span role="alert" className="text-[var(--color-err)]">{error}</span> : issue}
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

/** 转写内容区；当前显示哪一部分由转写 store 的 tab 决定。 */
export function TranscriptionView() {
  useModelCatalogRevision();
  const {
    tab,
    selectedFile,
    params,
    stage,
    statusText,
    errorMessage,
    result,
    editorCues,
    resultView,
    saveMessage,
    setSelectedFile,
    setParams,
    replaceParams,
    setRuntime,
  } = useTranscriptionStore();
  const providers = useProviderStore((s) => s.profiles);
  const effectiveAsrId = useProviderStore((s) => s.effective("asr"));
  const loadProviders = useProviderStore((s) => s.load);
  const updateProviderConfig = useProviderStore((s) => s.updateConfig);
  const asrProvider = providers.find((profile) => profile.id === effectiveAsrId);
  const hasApiKey = Boolean(asrProvider && providerReady(asrProvider, "asr"));
  const running = stage === "uploading" || stage === "recognizing";
  const { pickState, message, loadFileInfo, pickFile } = useFilePick(setSelectedFile);
  const dragActive = useFileDrop(loadFileInfo, tab === "transcribe");
  const textResult = useMemo(() => plainText(result), [result]);

  const toggleLanguageHint = (value: string) => {
    const next = params.languageHints.includes(value)
      ? params.languageHints.filter((item) => item !== value)
      : [...params.languageHints, value];
    setParams({ languageHints: next });
  };

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

  const statusTone: FileCardStatusTone =
    stage === "completed" ? "ok" : stage === "error" ? "err" : running ? "running" : "idle";
  const cardStatusText =
    statusText || (selectedFile ? "已就绪，可以开始识别。" : "等待选择文件。");

  return (
    <InspectorBody>
      {tab === "transcribe" ? (
        <>
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

          {result && (
            <section className="flex flex-col gap-2">
              <SectionHeader
                title="识别结果"
                right={
                  <Tabs
                    variant="view"
                    ariaLabel="识别结果视图"
                    tabs={[
                      { key: "text", label: "纯文本" },
                      { key: "subtitles", label: "字幕编辑" },
                    ]}
                    active={resultView}
                    onChange={(value) => setRuntime({ resultView: value })}
                  />
                }
              />
              {resultView === "text" ? (
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
              ) : (
                <>
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
                </>
              )}
            </section>
          )}
        </>
      ) : tab === "align" ? (
        <TranscriptAlignPanel />
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
