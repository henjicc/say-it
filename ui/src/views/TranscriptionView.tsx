import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import {
  useTranscriptionStore,
  type SelectedTranscriptionFile,
  type TranscriptionTab,
} from "@/store/useTranscriptionStore";

const MAX_FILE_SIZE = 2 * 1024 * 1024 * 1024;
const SUPPORTED_EXTENSIONS = [
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

const TABS: TabItem<TranscriptionTab>[] = [
  { key: "transcribe", label: "录音转写" },
  { key: "align", label: "文稿对齐" },
];

type PickState = "idle" | "loading" | "error";

function formatSize(size: number) {
  if (size >= 1024 * 1024 * 1024) return `${(size / 1024 / 1024 / 1024).toFixed(2)} GB`;
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${size} B`;
}

function extensionOf(name: string) {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function validateFile(file: SelectedTranscriptionFile) {
  const extension = extensionOf(file.name || file.path);
  if (file.size > MAX_FILE_SIZE) return "文件超过 2GB，Fun-ASR 录音文件识别可能无法处理。";
  if (!SUPPORTED_EXTENSIONS.includes(extension)) {
    return "文件扩展名不在 Fun-ASR 官方支持列表内，仍可尝试提交，以服务端结果为准。";
  }
  return "";
}

export function TranscriptionView() {
  const { tab, selectedFile, setTab, setSelectedFile } = useTranscriptionStore();
  const [pickState, setPickState] = useState<PickState>("idle");
  const [message, setMessage] = useState("");
  const [dragActive, setDragActive] = useState(false);

  const validationMessage = useMemo(
    () => (selectedFile ? validateFile(selectedFile) : ""),
    [selectedFile],
  );

  const loadFileInfo = async (path: string) => {
    setPickState("loading");
    setMessage("");
    try {
      const file = await cmd<SelectedTranscriptionFile>(CMD.getLocalFileInfo, { filePath: path });
      setSelectedFile(file);
      setMessage("");
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

  useEffect(() => {
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
          if (firstPath) void loadFileInfo(firstPath);
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
  }, []);

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
              <CardDescription>选择一个本地音视频文件，本阶段只记录文件信息，识别流程将在后续任务接入。</CardDescription>
            </div>
            {selectedFile && (
              <Button size="sm" onClick={pickFile} disabled={pickState === "loading"}>
                重新选择
              </Button>
            )}
          </div>

          <button
            type="button"
            onClick={pickFile}
            disabled={pickState === "loading"}
            className={cn(
              "mt-5 flex min-h-44 w-full flex-col items-center justify-center rounded-2xl border border-dashed px-6 py-8 text-center transition-colors",
              "focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--color-accent)_55%,transparent)]",
              dragActive
                ? "border-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_16%,transparent)]"
                : "border-white/16 bg-white/[0.035] hover:border-[color-mix(in_srgb,var(--color-accent)_42%,transparent)] hover:bg-white/[0.055]",
              pickState === "loading" && "cursor-wait opacity-75",
            )}
          >
            <span className="flex h-11 w-11 items-center justify-center rounded-full border border-white/12 bg-white/[0.06] text-white/70">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" className="h-5 w-5" aria-hidden>
                <path d="M12 16V4" />
                <path d="m7 9 5-5 5 5" />
                <path d="M5 18.5h14" />
              </svg>
            </span>
            <span className="mt-4 text-base font-medium text-white">
              {pickState === "loading" ? "正在读取文件信息…" : selectedFile ? selectedFile.name : "选择或拖放音视频文件"}
            </span>
            <span className="mt-2 max-w-xl text-sm leading-relaxed text-white/45">
              支持 mp3、wav、m4a、mp4、flac、ogg、webm 等常见格式，单文件最大 2GB。
            </span>
          </button>

          {selectedFile && (
            <div className="mt-4 grid gap-3 rounded-xl border border-white/10 bg-white/[0.035] p-4 text-sm md:grid-cols-[1fr_auto]">
              <div className="min-w-0">
                <p className="truncate font-medium text-white">{selectedFile.name}</p>
                <p className="mt-1 truncate text-white/42">{selectedFile.path}</p>
              </div>
              <div className="flex items-center gap-2 text-white/55 md:justify-end">
                <span>{formatSize(selectedFile.size)}</span>
                <span className="h-1 w-1 rounded-full bg-white/28" aria-hidden />
                <span>{extensionOf(selectedFile.name || selectedFile.path).toUpperCase() || "未知格式"}</span>
              </div>
            </div>
          )}

          {(validationMessage || message) && (
            <p className={cn("mt-3 text-sm", pickState === "error" ? "text-[#ff8589]" : "text-[#f5c56f]")}>
              {message || validationMessage}
            </p>
          )}
        </Card>
      ) : (
        <Card className="mt-2">
          <CardTitle>文稿对齐</CardTitle>
          <CardDescription>后续任务会在这里接入逐行文稿与词级时间戳对齐。</CardDescription>
        </Card>
      )}
    </div>
  );
}
