import { useEffect, useRef } from "react";
import { StatusDot } from "@/components/ui/StatusDot";
import type { CompareCellRuntime, CompareCellStatus } from "@/store/useCompareStore";

const STATUS_LABEL: Record<CompareCellStatus, string> = {
  idle: "等待中",
  queued: "等待录音结束",
  connecting: "连接中…",
  streaming: "识别中…",
  uploading: "上传中…",
  recognizing: "识别中…",
  done: "已完成",
  error: "出错",
};

const STATUS_TONE: Record<CompareCellStatus, "ok" | "err" | "warn" | "idle" | "rec"> = {
  idle: "idle",
  queued: "idle",
  connecting: "warn",
  streaming: "rec",
  uploading: "warn",
  recognizing: "warn",
  done: "ok",
  error: "err",
};

export function ResultCard({ runtime }: { runtime?: CompareCellRuntime }) {
  const status = runtime?.status || "idle";
  const text = runtime?.text || "";
  const bodyRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (status !== "streaming" || !bodyRef.current) return;
    bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
  }, [text, status]);

  return (
    <div className="relative">
      <div
        ref={bodyRef}
        className="h-48 overflow-y-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] p-3 pb-8 text-sm text-[var(--color-fg-muted)]"
      >
        {runtime?.errorMessage && <p className="mb-1 text-xs text-[var(--color-err)]">{runtime.errorMessage}</p>}
        {text || "—"}
      </div>
      <div className="pointer-events-none absolute bottom-2 right-2 flex items-center gap-1.5 rounded-[var(--radius-pill)] border border-[var(--color-line)] bg-[var(--color-surface-strong)] px-2 py-0.5">
        <StatusDot tone={STATUS_TONE[status]} />
        <span className="text-xs text-[var(--color-fg-subtle)]">{STATUS_LABEL[status]}</span>
      </div>
    </div>
  );
}
