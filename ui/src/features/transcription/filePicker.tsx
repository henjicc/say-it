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
  const name = file?.name || "录音识别结果";
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

export function FileDropSection(props: {
  file: SelectedTranscriptionFile | null;
  dragActive: boolean;
  disabled: boolean;
  pickState: PickState;
  message: string;
  onPick: () => void;
}) {
  const { file, dragActive, disabled, pickState, message, onPick } = props;
  const validationMessage = file ? validateFile(file) : "";

  return (
    <>
      <button
        type="button"
        onClick={onPick}
        disabled={disabled}
        className={cn(
          "flex min-h-52 w-full flex-col items-center justify-center rounded-[var(--radius-xl)] border border-dashed px-6 py-10 text-center transition-colors",
          "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
          dragActive
            ? "border-[var(--color-accent)] bg-[var(--accent-soft-strong)]"
            : "border-[var(--color-line-strong)] bg-[var(--color-surface)] hover:border-[var(--accent-ring)] hover:bg-[var(--color-surface-hover)]",
          disabled && "cursor-wait opacity-75",
        )}
      >
        <span className="flex h-12 w-12 items-center justify-center rounded-full border border-[var(--color-line)] bg-[var(--color-surface-strong)] text-[var(--color-fg-muted)]">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="h-5 w-5" aria-hidden>
            <path d="M12 16V4" />
            <path d="m7 9 5-5 5 5" />
            <path d="M5 18.5h14" />
          </svg>
        </span>
        <span className="mt-4 text-base font-medium text-[var(--color-fg)]">
          {pickState === "loading" ? "正在读取文件信息…" : file ? file.name : "选择或拖放音视频文件"}
        </span>
        <span className="mt-2 max-w-xl text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          支持 mp3、wav、m4a、mp4、flac、ogg、webm 等常见格式，单文件最大 2GB。
        </span>
      </button>

      {file && (
        <div className="grid gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4 text-sm md:grid-cols-[1fr_auto]">
          <div className="min-w-0">
            <p className="truncate font-medium text-[var(--color-fg)]">{file.name}</p>
            <p className="mt-1 truncate text-[var(--color-fg-subtle)]">{file.path}</p>
          </div>
          <div className="flex items-center gap-2 text-[var(--color-fg-muted)] md:justify-end">
            <span>{formatSize(file.size)}</span>
            <span className="h-1 w-1 rounded-full bg-[var(--color-fg-faint)]" aria-hidden />
            <span>{extensionOf(file.name || file.path).toUpperCase() || "未知格式"}</span>
          </div>
        </div>
      )}

      {(validationMessage || message) && (
        <p className={cn("text-sm", pickState === "error" ? "text-[var(--color-err)]" : "text-[var(--color-warn)]")}>
          {message || validationMessage}
        </p>
      )}
    </>
  );
}
