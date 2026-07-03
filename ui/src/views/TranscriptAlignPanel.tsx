import { useMemo } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
import { Tabs } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import {
  cancelAlignment,
  openProviderSettings,
  splitScriptLines,
  startAlignment,
} from "@/features/transcription/controller";
import {
  FileDropSection,
  defaultSrtName,
  useFileDrop,
  useFilePick,
} from "@/features/transcription/filePicker";
import { cuesFromAlignedLines, formatSrtTime, toSrt } from "@/features/transcription/subtitles";
import { useProviderStore } from "@/store/useProviderStore";
import { useTranscriptionStore, type AlignResultView } from "@/store/useTranscriptionStore";

/** 低于该匹配率的行视为与音频不符，黄色提示。 */
const LOW_MATCH_THRESHOLD = 0.6;

export function TranscriptAlignPanel() {
  const {
    alignFile,
    scriptText,
    alignStage,
    alignStatusText,
    alignErrorMessage,
    alignedLines,
    alignOptimizedCues,
    alignResultView,
    alignSaveMessage,
    setAlignFile,
    setScriptText,
    setRuntime,
  } = useTranscriptionStore();
  const providers = useProviderStore((s) => s.profiles);
  const hasApiKey = !!providers.find((profile) => profile.id === "funasr")?.status?.hasApiKey;

  const { pickState, message, loadFileInfo, pickFile } = useFilePick(setAlignFile);
  const running = alignStage === "uploading" || alignStage === "recognizing" || alignStage === "aligning";
  // 本面板仅在文稿对齐页签挂载，拖放天然只作用于当前页签
  const dragActive = useFileDrop(loadFileInfo);

  const lineCount = useMemo(() => splitScriptLines(scriptText).length, [scriptText]);
  const stats = useMemo(() => {
    if (!alignedLines || alignedLines.length === 0) return null;
    const avg = alignedLines.reduce((sum, line) => sum + line.matchRatio, 0) / alignedLines.length;
    const low = alignedLines.filter((line) => line.matchRatio < LOW_MATCH_THRESHOLD || line.interpolated).length;
    const replaced = (alignOptimizedCues || []).filter((cue) => cue.source === "asr").length;
    return { avg, low, replaced };
  }, [alignedLines, alignOptimizedCues]);

  const exportSrt = async () => {
    const cues =
      alignResultView === "optimized"
        ? alignOptimizedCues || []
        : alignedLines
          ? cuesFromAlignedLines(alignedLines)
          : [];
    if (cues.length === 0) {
      setRuntime({ alignSaveMessage: "当前没有可导出的字幕。" });
      return;
    }
    try {
      const path = await save({
        defaultPath: defaultSrtName(alignFile, alignResultView === "optimized" ? ".对齐修正" : ".对齐"),
        filters: [{ name: "SRT 字幕", extensions: ["srt"] }],
      });
      if (!path) return;
      await cmd(CMD.saveTextFile, { path, content: toSrt(cues) });
      setRuntime({ alignSaveMessage: `已导出：${path}` });
    } catch (error) {
      setRuntime({ alignSaveMessage: `导出失败：${String(error)}` });
    }
  };

  return (
    <Card className="mt-2">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <CardTitle>文稿对齐</CardTitle>
          <CardDescription>音频 + 一行一句的文稿，生成文本完全等于文稿、时间轴来自识别结果的字幕。</CardDescription>
        </div>
        {alignFile && (
          <Button size="sm" onClick={pickFile} disabled={pickState === "loading" || running}>
            重新选择
          </Button>
        )}
      </div>

      <FileDropSection
        file={alignFile}
        dragActive={dragActive}
        disabled={pickState === "loading" || running}
        pickState={pickState}
        message={message}
        onPick={pickFile}
      />

      <div className="mt-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <p className="text-xs font-medium text-white/60">文稿（一行一句，字幕文本将完全按照文稿输出）</p>
          <span className="text-xs tabular-nums text-white/40">{lineCount} 行有效文稿</span>
        </div>
        <textarea
          value={scriptText}
          onChange={(event) => setScriptText(event.target.value)}
          disabled={running}
          placeholder={"把文稿粘贴到这里，一行一句。\n空行会被自动忽略。"}
          className="mt-2 min-h-48 w-full resize-y rounded-xl border border-white/10 bg-white/[0.035] px-4 py-3 text-sm leading-7 text-white/82 outline-none focus:border-[color-mix(in_srgb,var(--color-accent)_45%,transparent)] disabled:opacity-60"
        />
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Button variant="primary" onClick={startAlignment} disabled={!alignFile || lineCount === 0 || running}>
          开始对齐
        </Button>
        {(alignStage === "uploading" || alignStage === "recognizing") && (
          <Button variant="danger" onClick={cancelAlignment}>
            取消
          </Button>
        )}
        {!hasApiKey && <Button onClick={openProviderSettings}>去设置 API Key</Button>}
      </div>

      <div className="mt-4 rounded-xl border border-white/10 bg-white/[0.03] p-4">
        <div className="flex items-center gap-3">
          <span
            className={cn(
              "h-2.5 w-2.5 rounded-full",
              alignStage === "completed" && "bg-[#25c36f]",
              alignStage === "error" && "bg-[#ff6b6b]",
              running && "animate-pulse bg-[var(--color-accent)]",
              alignStage === "idle" && "bg-white/28",
            )}
            aria-hidden
          />
          <p className="text-sm text-white/70">{alignStatusText || "选择音频并粘贴文稿后开始。"}</p>
        </div>
        {alignErrorMessage && <p className="mt-2 text-sm text-[#ff8589]">{alignErrorMessage}</p>}
        <p className="mt-2 text-xs text-white/40">
          识别参数沿用「通用设置」页签中的设置；同一文件重复执行时会复用上次识别结果，只重新对齐。
        </p>
      </div>

      {alignedLines && alignedLines.length > 0 && (
        <div className="mt-5 border-t border-white/10 pt-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <Tabs<AlignResultView>
              tabs={[
                { key: "script", label: "完全按文稿" },
                { key: "optimized", label: `识别修正${stats && stats.replaced > 0 ? `（${stats.replaced} 处）` : ""}` },
              ]}
              active={alignResultView}
              onChange={(value) => setRuntime({ alignResultView: value })}
            />
            <Button size="sm" onClick={exportSrt}>
              导出 SRT
            </Button>
          </div>

          {alignResultView === "script" ? (
            <>
              <p className="mt-3 text-sm text-white/60">
                共 {alignedLines.length} 行，平均匹配率 {stats ? Math.round(stats.avg * 100) : 0}%
                {stats && stats.low > 0 && (
                  <span className="text-[#f5c56f]">，{stats.low} 行匹配度低（已黄色标注，时间为估算）</span>
                )}
              </p>
              <div className="mt-3 max-h-[34rem] overflow-auto rounded-xl border border-white/10 bg-white/[0.035]">
                {alignedLines.map((line, index) => {
                  const low = line.matchRatio < LOW_MATCH_THRESHOLD || line.interpolated;
                  return (
                    <div
                      key={line.lineIndex}
                      className={cn(
                        "grid gap-2 border-b border-white/8 px-4 py-3 last:border-b-0 md:grid-cols-[3rem_15rem_4.5rem_1fr]",
                        low && "bg-[#f5c56f]/[0.08]",
                      )}
                    >
                      <span className="text-xs tabular-nums text-white/35">{index + 1}</span>
                      <span className="font-mono text-xs text-white/50">
                        {formatSrtTime(line.beginMs)} → {formatSrtTime(line.endMs)}
                      </span>
                      <span
                        className={cn("text-xs tabular-nums", low ? "text-[#f5c56f]" : "text-white/45")}
                        title={low ? "该行与音频匹配度低，时间为估算，建议核对" : undefined}
                      >
                        {line.interpolated ? "估算" : `${Math.round(line.matchRatio * 100)}%`}
                      </span>
                      <span className="text-sm leading-6 text-white/82">{line.text}</span>
                    </div>
                  );
                })}
              </div>
            </>
          ) : (
            <>
              <p className="mt-3 text-sm text-white/60">
                {stats && stats.replaced > 0 ? (
                  <>
                    与音频差异过大的片段已改用识别文本，音频中文稿没写但确实说了的内容也已补入（
                    <span className="text-[#6fc7f5]">蓝色标注</span>
                    ，共 {stats.replaced} 处），其余部分仍完全按文稿输出，不再死板按整行取舍。
                  </>
                ) : (
                  "所有文稿内容与音频匹配良好，无需修正，与「完全按文稿」结果一致。"
                )}
              </p>
              <div className="mt-3 max-h-[34rem] overflow-auto rounded-xl border border-white/10 bg-white/[0.035]">
                {(alignOptimizedCues || []).map((cue) => (
                  <div
                    key={cue.index}
                    className={cn(
                      "grid gap-2 border-b border-white/8 px-4 py-3 last:border-b-0 md:grid-cols-[3rem_15rem_4.5rem_1fr]",
                      cue.source === "asr" && "bg-[#6fc7f5]/[0.08]",
                    )}
                  >
                    <span className="text-xs tabular-nums text-white/35">{cue.index}</span>
                    <span className="font-mono text-xs text-white/50">
                      {formatSrtTime(cue.beginMs)} → {formatSrtTime(cue.endMs)}
                    </span>
                    <span
                      className={cn("text-xs tabular-nums", cue.source === "asr" ? "text-[#6fc7f5]" : "text-white/45")}
                      title={cue.source === "asr" ? "该段内容来自识别文本：或是文稿与音频差异过大被替换，或是音频里说了但文稿没写" : undefined}
                    >
                      {cue.source === "asr" ? "识别" : `${Math.round((cue.matchRatio ?? 0) * 100)}%`}
                    </span>
                    <span className="text-sm leading-6 text-white/82">{cue.text}</span>
                  </div>
                ))}
                {(alignOptimizedCues || []).length === 0 && (
                  <p className="p-4 text-sm text-white/45">修正结果为空：文稿与音频均无可保留内容。</p>
                )}
              </div>
            </>
          )}

          {alignSaveMessage && <p className="mt-2 text-xs text-white/45">{alignSaveMessage}</p>}
        </div>
      )}
    </Card>
  );
}
