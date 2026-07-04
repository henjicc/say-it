import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import type { SelectedTranscriptionFile } from "@/store/useTranscriptionStore";

export const MAX_FILE_SIZE = 2 * 1024 * 1024 * 1024;
export const SUPPORTED_EXTENSIONS = [
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

export type PickState = "idle" | "loading" | "error";

export function formatSize(size: number) {
  if (size >= 1024 * 1024 * 1024) return `${(size / 1024 / 1024 / 1024).toFixed(2)} GB`;
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${size} B`;
}

export function extensionOf(name: string) {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export function validateFile(file: SelectedTranscriptionFile) {
  const extension = extensionOf(file.name || file.path);
  if (file.size > MAX_FILE_SIZE) return "文件超过 2GB，Fun-ASR 录音文件识别可能无法处理。";
  if (!SUPPORTED_EXTENSIONS.includes(extension)) {
    return "文件扩展名不在 Fun-ASR 官方支持列表内，仍可尝试提交，以服务端结果为准。";
  }
  return "";
}

export function defaultSrtName(file: SelectedTranscriptionFile | null, suffix = "") {
  const name = file?.name || "字幕转写结果";
  const dot = name.lastIndexOf(".");
  return `${dot > 0 ? name.slice(0, dot) : name}${suffix}.srt`;
}

export function useFilePick(onFile: (file: SelectedTranscriptionFile) => void) {
  const [pickState, setPickState] = useState<PickState>("idle");
  const [message, setMessage] = useState("");
  const onFileRef = useRef(onFile);
  onFileRef.current = onFile;

  const loadFileInfo = async (path: string) => {
    setPickState("loading");
    setMessage("");
    try {
      const file = await cmd<SelectedTranscriptionFile>(CMD.getLocalFileInfo, { filePath: path });
      onFileRef.current(file);
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

  return { pickState, message, loadFileInfo, pickFile };
}

/** 监听 webview 拖放；`enabled` 用于按当前页签路由拖放目标。 */
export function useFileDrop(onPath: (path: string) => void, enabled = true) {
  const [dragActive, setDragActive] = useState(false);
  const onPathRef = useRef(onPath);
  onPathRef.current = onPath;

  useEffect(() => {
    if (!enabled) {
      setDragActive(false);
      return;
    }
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
          if (firstPath) onPathRef.current(firstPath);
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
  }, [enabled]);

  return dragActive;
}

const statusDotClass: Record<FileCardStatusTone, string> = {
  idle: "bg-[var(--color-fg-faint)]",
  running: "bg-[var(--color-accent)] animate-pulse",
  ok: "bg-[var(--color-ok)]",
  err: "bg-[var(--color-err)]",
};

const statusTextClass: Record<FileCardStatusTone, string> = {
  idle: "text-[var(--color-fg-subtle)]",
  running: "text-[var(--color-fg-muted)]",
  ok: "text-[var(--color-ok)]",
  err: "text-[var(--color-err)]",
};

export type FileCardStatusTone = "idle" | "running" | "ok" | "err";

/**
 * 文件卡：未选文件时是拖放区，选中后合并展示文件信息、任务状态、
 * 不确定态进度条与主操作，替代原先「拖放区 + 信息卡 + 独立状态条」三段式。
 */
export function FileCard(props: {
  file: SelectedTranscriptionFile | null;
  dragActive: boolean;
  disabled: boolean;
  pickState: PickState;
  message: string;
  onPick: () => void;
  statusTone: FileCardStatusTone;
  statusText: string;
  errorMessage?: string;
  /** 状态行下方的补充说明（保存提示、参数说明等）。 */
  hint?: React.ReactNode;
  /** 右下角主操作（开始识别 / 取消 / 去设置 API Key 等）。 */
  actions?: React.ReactNode;
}) {
  const { file, dragActive, disabled, pickState, message, onPick, statusTone, statusText, errorMessage, hint, actions } = props;
  const validationMessage = file ? validateFile(file) : "";
  const running = statusTone === "running";

  if (!file) {
    return (
      <div className="flex flex-col gap-2">
        <button
          type="button"
          onClick={onPick}
          disabled={disabled}
          className={cn(
            "flex min-h-44 w-full flex-col items-center justify-center rounded-[var(--radius-xl)] border border-dashed px-6 py-8 text-center transition-colors",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
            dragActive
              ? "border-[var(--color-accent)] bg-[var(--accent-soft-strong)]"
              : "border-[var(--color-line-strong)] bg-[var(--color-surface)] hover:border-[var(--accent-ring)] hover:bg-[var(--color-surface-hover)]",
            disabled && "cursor-wait opacity-75",
          )}
        >
          <span className="flex h-11 w-11 items-center justify-center rounded-full border border-[var(--color-line)] bg-[var(--color-surface-strong)] text-[var(--color-fg-muted)]">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="h-5 w-5" aria-hidden>
              <path d="M12 16V4" />
              <path d="m7 9 5-5 5 5" />
              <path d="M5 18.5h14" />
            </svg>
          </span>
          <span className="mt-3 text-base font-medium text-[var(--color-fg)]">
            {pickState === "loading" ? "正在读取文件信息…" : "选择或拖放音视频文件"}
          </span>
          <span className="mt-1.5 max-w-xl text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            支持 mp3、wav、m4a、mp4、flac、ogg、webm 等常见格式，单文件最大 2GB。
          </span>
        </button>
        {message && <p className="text-sm text-[var(--color-err)]">{message}</p>}
      </div>
    );
  }

  return (
    <div
      className={cn(
        "rounded-[var(--radius-lg)] border bg-[var(--color-surface)] transition-colors",
        dragActive ? "border-[var(--color-accent)] bg-[var(--accent-soft)]" : "border-[var(--color-line)]",
      )}
    >
      {/* 文件信息行 */}
      <div className="flex items-center gap-3 px-4 py-3">
        <span className="flex h-10 w-10 flex-none items-center justify-center rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface-strong)] text-[var(--color-fg-muted)]">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" className="h-4.5 w-4.5" aria-hidden>
            <path d="M9 18V6l10-2v11" />
            <circle cx="6.5" cy="18" r="2.5" />
            <circle cx="16.5" cy="15" r="2.5" />
          </svg>
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-[var(--color-fg)]">
            {pickState === "loading" ? "正在读取文件信息…" : file.name}
          </p>
          <p className="mt-0.5 truncate text-xs text-[var(--color-fg-subtle)]">
            {formatSize(file.size)}
            <span className="mx-1.5 text-[var(--color-fg-faint)]">·</span>
            {extensionOf(file.name || file.path).toUpperCase() || "未知格式"}
            <span className="mx-1.5 text-[var(--color-fg-faint)]">·</span>
            {file.path}
          </p>
        </div>
      </div>

      {(validationMessage || message) && (
        <p className={cn("px-4 pb-2 text-xs", pickState === "error" ? "text-[var(--color-err)]" : "text-[var(--color-warn)]")}>
          {message || validationMessage}
        </p>
      )}

      {/* 进度条：任务进行中显示不确定态动画 */}
      {running && (
        <div className="mx-4 mb-1 h-1 overflow-hidden rounded-[var(--radius-pill)] bg-[var(--color-surface-strong)]">
          <span className="block h-full w-1/3 rounded-[var(--radius-pill)] bg-[var(--color-accent)] animate-[progress-indeterminate_1.4s_ease-in-out_infinite]" />
        </div>
      )}

      {/* 状态 + 操作行 */}
      <div className="flex flex-wrap items-center gap-3 border-t border-[var(--color-line)] px-4 py-3">
        <span className={cn("h-2 w-2 flex-none rounded-full", statusDotClass[statusTone])} aria-hidden />
        <div className="min-w-0 flex-1">
          <p className={cn("text-sm", statusTextClass[statusTone])}>{statusText}</p>
          {errorMessage && <p className="mt-0.5 text-xs text-[var(--color-err)]">{errorMessage}</p>}
          {hint && <p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">{hint}</p>}
        </div>
        {actions && <span className="flex flex-none flex-wrap items-center gap-2">{actions}</span>}
      </div>
    </div>
  );
}
