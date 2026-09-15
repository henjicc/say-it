import { useCallback, useEffect, useMemo } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { CheckField, Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { FormGrid } from "@/components/ui/FormGrid";
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
import { useTranscriptionParamsSync } from "@/features/transcription/paramsSync";
import { FILE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ModelPicker } from "@/features/models/ModelPicker";
import { TranscriptAlignPanel } from "@/views/TranscriptAlignPanel";
import { useProviderStore } from "@/store/useProviderStore";
import {
  useTranscriptionStore,
  type TranscriptionParams,
  type TranscriptionTab,
} from "@/store/useTranscriptionStore";

const TABS: TabItem<TranscriptionTab>[] = [
  { key: "transcribe", label: "字幕转写" },
  { key: "align", label: "文稿对齐" },
  { key: "settings", label: "通用设置" },
];

const LANGUAGE_OPTIONS = [
  { value: "zh", label: "中文" },
  { value: "en", label: "英文" },
  { value: "ja", label: "日语" },
];

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
    setTab,
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
  const hasApiKey = !!asrProvider?.status?.hasApiKey;
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
    <div className="flex flex-col gap-7">
      <PageHeader
        title="字幕转写"
        description="处理本地音视频文件，生成转写文本或用于文稿对齐的时间轴。"
      />

      <Tabs<TranscriptionTab> tabs={TABS} active={tab} onChange={setTab} />

      {tab === "transcribe" ? (
        <>
          <SettingsSection title="音视频文件">
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
          </SettingsSection>

          {result && (
            <SettingsSection
              title="识别结果"
              right={
                <Tabs
                  tabs={[
                    { key: "text", label: "纯文本" },
                    { key: "subtitles", label: "字幕编辑" },
                  ]}
                  active={resultView}
                  onChange={(value) => setRuntime({ resultView: value })}
                />
              }
            >
              {resultView === "text" ? (
                <>
                  <textarea
                    readOnly
                    value={textResult}
                    className="min-h-72 w-full resize-y rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3 text-sm leading-7 text-[var(--color-fg-muted)] outline-none"
                  />
                  <div className="flex items-center gap-3">
                    <Button size="sm" onClick={() => copyText(textResult)}>
                      复制文本
                    </Button>
                    {saveMessage && <span className="text-xs text-[var(--color-fg-subtle)]">{saveMessage}</span>}
                  </div>
                </>
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
                  {saveMessage && <p className="text-xs text-[var(--color-fg-subtle)]">{saveMessage}</p>}
                </>
              )}
            </SettingsSection>
          )}
        </>
      ) : tab === "align" ? (
        <TranscriptAlignPanel />
      ) : (
        <SettingsSection title="通用设置">
          <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            字幕转写与文稿对齐共用这些识别设置。
          </p>

          <FormGrid>
            <Field label="识别模型" controlId="transcription-asr-model">
              <ModelPicker
                id="transcription-asr-model"
                value={params.model}
                options={FILE_ASR_MODEL_OPTIONS}
                panelLabel="选择语音识别模型"
                onChange={(value) => setParams({ model: value })}
              />
            </Field>
          </FormGrid>

          <div>
            <p className="text-xs font-medium text-[var(--color-fg-muted)]">语种提示</p>
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

          <FormGrid className="items-center">
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
          </FormGrid>

          {saveMessage && <p className="text-xs text-[var(--color-fg-subtle)]">{saveMessage}</p>}
        </SettingsSection>
      )}
    </div>
  );
}
