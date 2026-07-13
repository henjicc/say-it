import { useMemo } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Tabs } from "@/components/ui/Tabs";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { SubtitleEditor } from "@/components/transcription/SubtitleEditor";
import { copyText } from "@/lib/clipboard";
import { CMD, cmd } from "@/lib/tauri";
import {
  cancelAlignment,
  openProviderSettings,
  splitScriptLines,
  startAlignment,
} from "@/features/transcription/controller";
import {
  FileCard,
  type FileCardStatusTone,
  defaultSrtName,
  useFileDrop,
  useFilePick,
} from "@/features/transcription/filePicker";
import {
  LOW_MATCH_THRESHOLD,
  editablePlainText,
} from "@/features/transcription/subtitles";
import { useProviderStore } from "@/store/useProviderStore";
import { useTranscriptionStore, type AlignResultView } from "@/store/useTranscriptionStore";

export function TranscriptAlignPanel() {
  const {
    alignFile,
    scriptText,
    alignStage,
    alignStatusText,
    alignErrorMessage,
    alignedLines,
    alignEditorCues,
    alignResultView,
    alignSaveMessage,
    setAlignFile,
    setScriptText,
    setRuntime,
  } = useTranscriptionStore();
  const providers = useProviderStore((s) => s.profiles);
  const effectiveAsrId = useProviderStore((s) => s.effective("asr"));
  const hasApiKey = !!providers.find((profile) => profile.id === effectiveAsrId)?.status?.hasApiKey;

  const { pickState, message, loadFileInfo, pickFile } = useFilePick(setAlignFile);
  const running = alignStage === "uploading" || alignStage === "recognizing" || alignStage === "aligning";
  const cancellable = alignStage === "uploading" || alignStage === "recognizing";
  // 本面板仅在文稿对齐页签挂载，拖放天然只作用于当前页签
  const dragActive = useFileDrop(loadFileInfo);

  const lineCount = useMemo(() => splitScriptLines(scriptText).length, [scriptText]);
  const stats = useMemo(() => {
    if (!alignedLines || alignedLines.length === 0) return null;
    const avg = alignedLines.reduce((sum, line) => sum + line.matchRatio, 0) / alignedLines.length;
    const low = alignedLines.filter((line) => line.matchRatio < LOW_MATCH_THRESHOLD || line.interpolated).length;
    const replaced = (alignEditorCues?.optimized || []).filter((cue) => cue.badge?.tone === "accent").length;
    return { avg, low, replaced };
  }, [alignedLines, alignEditorCues]);

  const currentCues = alignEditorCues ? alignEditorCues[alignResultView] : null;

  const updateCurrentCues = (next: NonNullable<typeof currentCues>) => {
    if (!alignEditorCues) return;
    setRuntime({ alignEditorCues: { ...alignEditorCues, [alignResultView]: next } });
  };

  const exportSrt = async () => {
    if (!currentCues || currentCues.length === 0) {
      setRuntime({ alignSaveMessage: "当前没有可导出的字幕。" });
      return;
    }
    try {
      const path = await save({
        defaultPath: defaultSrtName(alignFile, alignResultView === "optimized" ? ".对齐修正" : ".对齐"),
        filters: [{ name: "SRT 字幕", extensions: ["srt"] }],
      });
      if (!path) return;
      await cmd(CMD.saveSubtitleSrt, { path, cues: currentCues });
      setRuntime({ alignSaveMessage: `已导出：${path}` });
    } catch (error) {
      setRuntime({ alignSaveMessage: `导出失败：${String(error)}` });
    }
  };

  const statusTone: FileCardStatusTone =
    alignStage === "completed" ? "ok" : alignStage === "error" ? "err" : running ? "running" : "idle";
  const cardStatusText =
    alignStatusText || (alignFile ? (lineCount > 0 ? "已就绪，可以开始对齐。" : "请在下方粘贴一行一句的文稿。") : "选择音频并粘贴文稿后开始。");

  return (
    <>
      <SettingsSection title="音视频文件">
        <FileCard
          file={alignFile}
          dragActive={dragActive}
          disabled={pickState === "loading" || running}
          pickState={pickState}
          message={message}
          onPick={pickFile}
          statusTone={statusTone}
          statusText={cardStatusText}
          errorMessage={alignErrorMessage}
          hint="识别参数沿用「通用设置」页签；同一文件重复执行时复用上次识别结果，只重新对齐。"
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
              {cancellable ? (
                <Button size="sm" variant="danger" onClick={cancelAlignment}>
                  取消
                </Button>
              ) : (
                <Button size="sm" variant="primary" onClick={startAlignment} disabled={!alignFile || lineCount === 0 || running}>
                  开始对齐
                </Button>
              )}
            </>
          }
        />
      </SettingsSection>

      <SettingsSection
        title="文稿"
        right={<span className="text-xs tabular-nums text-[var(--color-fg-subtle)]">{lineCount} 行有效文稿</span>}
      >
        <textarea
          value={scriptText}
          onChange={(event) => setScriptText(event.target.value)}
          disabled={running}
          placeholder={"把文稿粘贴到这里，一行一句，字幕文本将完全按照文稿输出。\n空行会被自动忽略。"}
          className="min-h-44 w-full resize-y rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3 text-sm leading-7 text-[var(--color-fg-muted)] outline-none focus:border-[var(--accent-ring)] disabled:opacity-60"
        />
      </SettingsSection>

      {alignEditorCues && (
        <SettingsSection
          title="对齐结果"
          right={
            <Tabs<AlignResultView>
              tabs={[
                { key: "script", label: "完全按文稿" },
                { key: "optimized", label: `识别修正${stats && stats.replaced > 0 ? `（${stats.replaced} 处）` : ""}` },
              ]}
              active={alignResultView}
              onChange={(value) => setRuntime({ alignResultView: value })}
            />
          }
        >
          <p className="text-sm leading-relaxed text-[var(--color-fg-muted)]">
            {alignResultView === "script" ? (
              <>
                共 {alignedLines?.length ?? 0} 行，平均匹配率 {stats ? Math.round(stats.avg * 100) : 0}%
                {stats && stats.low > 0 && (
                  <span className="text-[var(--color-warn)]">，{stats.low} 行匹配度低（已黄色标注，时间为估算）</span>
                )}
                。
              </>
            ) : stats && stats.replaced > 0 ? (
              <>
                与音频差异过大的片段已改用识别文本，音频中文稿没写但确实说了的内容也已补入（
                <span className="text-[var(--color-accent-light)]">「识别」标注</span>
                ，共 {stats.replaced} 处），其余部分仍完全按文稿输出。
              </>
            ) : (
              "所有文稿内容与音频匹配良好，无需修正，与「完全按文稿」结果一致。"
            )}
          </p>

          <SubtitleEditor
            mediaPath={alignFile?.path ?? null}
            cues={currentCues ?? []}
            onCuesChange={updateCurrentCues}
            footer={
              <>
                <Button size="sm" onClick={() => copyText(editablePlainText(currentCues ?? []))}>
                  复制文本
                </Button>
                <Button size="sm" variant="primary" onClick={exportSrt} disabled={!currentCues || currentCues.length === 0}>
                  导出 SRT
                </Button>
              </>
            }
          />

          {alignSaveMessage && <p className="text-xs text-[var(--color-fg-subtle)]">{alignSaveMessage}</p>}
        </SettingsSection>
      )}
    </>
  );
}
