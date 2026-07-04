import { useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { StatusBar } from "@/components/ui/StatusBar";
import { ModelGrid } from "@/components/compare/ModelGrid";
import { SourcePicker } from "@/components/compare/SourcePicker";
import { hardAbortCompare, startCompare, stopCompare } from "@/features/compare/controller";
import { useCompareStore, type ComparePhase } from "@/store/useCompareStore";

const PHASE_LABEL: Record<ComparePhase, string> = {
  idle: "等待开始",
  recording: "录音中…",
  playing: "播放识别中…",
  finalizing: "正在收尾…",
};

const MAIN_LABEL: Record<ComparePhase, string> = {
  idle: "开始对比",
  recording: "停止录音",
  playing: "停止对比",
  finalizing: "停止",
};

export function SettingsComparePanel() {
  const phase = useCompareStore((s) => s.phase);
  const globalError = useCompareStore((s) => s.globalError);
  const playbackProgress = useCompareStore((s) => s.playbackProgress);
  const sourceMode = useCompareStore((s) => s.prefs.sourceMode);

  useEffect(() => {
    return () => {
      void hardAbortCompare();
    };
  }, []);

  const onMainClick = () => {
    if (phase === "idle") void startCompare();
    else void stopCompare();
  };

  return (
    <div className="flex flex-col gap-6">
      <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
        同一段录音或音频文件，同时喂给多个模型识别，并排比较效果。每个模型各占一格，多个实时模型会同时并发消耗对应的
        API 调用额度，请留意用量。
      </p>

      <SourcePicker />
      <ModelGrid />

      {globalError && <StatusBar tone="err" message={globalError} variant="inline" />}
      {phase !== "idle" && <StatusBar tone={phase === "finalizing" ? "info" : "running"} message={PHASE_LABEL[phase]} />}

      {sourceMode === "upload" && playbackProgress && playbackProgress.durationMs > 0 && (
        <div className="h-1.5 overflow-hidden rounded-[var(--radius-pill)] bg-[var(--color-surface-strong)]">
          <div
            className="h-full rounded-[var(--radius-pill)] bg-[var(--color-accent)] transition-[width]"
            style={{ width: `${Math.min(100, (playbackProgress.currentMs / playbackProgress.durationMs) * 100)}%` }}
          />
        </div>
      )}

      <div>
        <Button variant="primary" onClick={onMainClick}>
          {MAIN_LABEL[phase]}
        </Button>
      </div>
    </div>
  );
}
