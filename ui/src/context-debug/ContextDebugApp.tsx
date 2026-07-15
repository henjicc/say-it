import { useState } from "react";
import { Crosshair, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { CMD, EVT, cmd } from "@/lib/tauri";
import { cn } from "@/lib/cn";

type CaptureStatus =
  | "captured"
  | "empty"
  | "blocked"
  | "sensitive"
  | "timedOut"
  | "unsupported"
  | "failed";

interface DebugResult {
  status: CaptureStatus;
  captureMethod: "nativeText" | "ocr";
  source?: "ia2Text" | "uiaTextPattern" | "win32Message" | "officeNative" | "msaa" | "clipboardDeep" | "ocr" | null;
  appName: string;
  processName: string;
  processId: number;
  windowTitle?: string | null;
  selectedText?: string | null;
  focusedText?: string | null;
  caretContext?: string | null;
  visibleText: string[];
  documentText: string[];
  ocrText: string[];
  ocrBlocks: OcrTextBlock[];
  screenshotWidth: number;
  screenshotHeight: number;
  screenshotElapsedMs: number;
  modelInitMs: number;
  ocrElapsedMs: number;
  screenshotDataUrl?: string | null;
  diagnostics: string[];
  elapsedMs: number;
  truncated: boolean;
  formattedContext: string;
  message?: string | null;
}

interface NormalizedRegion {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

interface OcrTextBlock {
  text: string;
  confidence: number;
  bounds: NormalizedRegion;
}

const STATUS: Record<CaptureStatus, { label: string; tone: string }> = {
  captured: { label: "捕获成功", tone: "text-[var(--color-ok)]" },
  empty: { label: "没有可用内容", tone: "text-[var(--color-warn)]" },
  blocked: { label: "已被黑名单阻止", tone: "text-[var(--color-warn)]" },
  sensitive: { label: "敏感输入区域，已停止读取", tone: "text-[var(--color-warn)]" },
  timedOut: { label: "捕获超时", tone: "text-[var(--color-err)]" },
  unsupported: { label: "当前系统不支持", tone: "text-[var(--color-err)]" },
  failed: { label: "捕获失败", tone: "text-[var(--color-err)]" },
};

function ResultSection({ title, value }: { title: string; value?: string | null }) {
  return (
    <section className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
      <h2 className="text-xs font-medium text-[var(--color-fg-muted)]">{title}</h2>
      <pre className="mt-2 max-h-52 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-5 text-[var(--color-fg)]">
        {value || "（空）"}
      </pre>
    </section>
  );
}

export function ContextDebugApp() {
  const [result, setResult] = useState<DebugResult>();
  const [capturing, setCapturing] = useState(false);

  useTauriEvent<{ state?: "waiting" | "capturing" }>(EVT.contextDebugState, (payload) => {
    setCapturing(payload.state === "capturing");
    if (payload.state === "waiting") setResult(undefined);
  });
  useTauriEvent<DebugResult>(EVT.contextDebugResult, (payload) => {
    setResult(payload);
    setCapturing(false);
  });

  const close = async () => {
    await cmd(CMD.closeActiveAppContextDebug);
  };

  const status = result ? STATUS[result.status] : undefined;
  return (
    <div className="flex h-screen flex-col overflow-hidden border border-[var(--color-line-strong)] bg-[var(--color-bg)]">
      <header
        data-tauri-drag-region
        className="flex h-11 shrink-0 items-center justify-between border-b border-[var(--color-line)] bg-[var(--color-bg-titlebar)] px-3"
      >
        <div data-tauri-drag-region className="flex items-center gap-2 text-sm font-medium">
          <Crosshair className="h-4 w-4 text-[var(--color-accent-light)]" strokeWidth={1.8} aria-hidden />
          当前软件上下文调试
        </div>
        <IconButton label="关闭上下文调试" className="no-drag" onClick={() => void close()}>
          <X className="h-4 w-4" strokeWidth={1.8} aria-hidden />
        </IconButton>
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto p-5">
        <section className="rounded-[var(--radius-lg)] border border-[var(--accent-ring)] bg-[var(--accent-soft)] p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="text-sm font-medium text-[var(--color-fg)]">
                {capturing ? "正在读取当前窗口…" : "点击目标应用后按下快捷键"}
              </p>
              <p className="mt-1 text-xs leading-5 text-[var(--color-fg-subtle)]">
                调试窗口会保持置顶。先在其他软件中点击目标输入区，再按快捷键捕获一次；结果仅在本机显示，不会调用模型或保存。
              </p>
            </div>
            <kbd className="rounded-[var(--radius-md)] border border-[var(--color-line-strong)] bg-[var(--color-bg)] px-3 py-2 font-mono text-xs text-[var(--color-accent-light)]">
              Ctrl + Shift + F8
            </kbd>
          </div>
        </section>

        <div className="mt-4 flex items-center justify-between gap-3">
          <p className={cn("text-sm", status?.tone ?? "text-[var(--color-fg-subtle)]")}>
            {capturing ? "正在捕获" : status?.label ?? "等待首次捕获"}
          </p>
          <Button size="sm" disabled={!result || capturing} onClick={() => setResult(undefined)}>
            <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
            清空结果
          </Button>
        </div>

        {result && (
          <div className="mt-4 flex flex-col gap-3">
            {result.message && (
              <p role="alert" className="rounded-[var(--radius-md)] bg-[color-mix(in_srgb,var(--color-rec)_12%,transparent)] px-3 py-2 text-xs text-[var(--color-err)]">
                {result.message}
              </p>
            )}
            {result.diagnostics.length > 0 && (
              <ResultSection title="捕获诊断" value={result.diagnostics.join("\n")} />
            )}
            <section className="grid grid-cols-2 gap-x-5 gap-y-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] p-4 text-xs">
              <div><span className="text-[var(--color-fg-faint)]">应用</span><p className="mt-1 break-words text-[var(--color-fg)]">{result.appName || "—"}</p></div>
              <div><span className="text-[var(--color-fg-faint)]">进程</span><p className="mt-1 break-words text-[var(--color-fg)]">{result.processName || "—"} {result.processId ? `(PID ${result.processId})` : ""}</p></div>
              <div className="col-span-2"><span className="text-[var(--color-fg-faint)]">窗口标题</span><p className="mt-1 break-words text-[var(--color-fg)]">{result.windowTitle || "—"}</p></div>
              <div><span className="text-[var(--color-fg-faint)]">模式</span><p className="mt-1 text-[var(--color-fg)]">{result.captureMethod === "ocr" ? "窗口 OCR" : "文本提取"}</p></div>
              <div><span className="text-[var(--color-fg-faint)]">来源</span><p className="mt-1 text-[var(--color-fg)]">{result.source || "—"}</p></div>
              <div><span className="text-[var(--color-fg-faint)]">总耗时</span><p className="mt-1 text-[var(--color-fg)]">{result.elapsedMs} ms</p></div>
              {result.captureMethod === "ocr" && <div><span className="text-[var(--color-fg-faint)]">图像</span><p className="mt-1 text-[var(--color-fg)]">{result.screenshotWidth && result.screenshotHeight ? `${result.screenshotWidth} × ${result.screenshotHeight}` : "—"}</p></div>}
              {result.captureMethod === "ocr" && <div><span className="text-[var(--color-fg-faint)]">分项耗时</span><p className="mt-1 text-[var(--color-fg)]">截图 {result.screenshotElapsedMs} ms · 模型初始化 {result.modelInitMs} ms · OCR {result.ocrElapsedMs} ms</p></div>}
              {result.captureMethod === "ocr" && <div><span className="text-[var(--color-fg-faint)]">OCR 文字框</span><p className="mt-1 text-[var(--color-fg)]">{result.ocrBlocks.length} 个</p></div>}
              <div><span className="text-[var(--color-fg-faint)]">结果</span><p className="mt-1 text-[var(--color-fg)]">{result.truncated ? "已按上下文预算裁剪" : "完整"}</p></div>
            </section>

            {result.captureMethod === "ocr" && result.screenshotDataUrl && (
              <section className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
                <div className="flex items-center justify-between gap-3">
                  <h2 className="text-xs font-medium text-[var(--color-fg-muted)]">激活窗口整窗截图与 OCR 文字框</h2>
                </div>
                <div className="relative mt-3 overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line-strong)] bg-black">
                  <img src={result.screenshotDataUrl} alt="当前激活窗口内存截图" className="block h-auto w-full" />
                  {result.ocrBlocks.map((block, index) => (
                    <div
                      key={`${block.text}-${index}`}
                      title={`${block.text} · ${(block.confidence * 100).toFixed(1)}% · ${block.bounds.left.toFixed(3)},${block.bounds.top.toFixed(3)},${block.bounds.right.toFixed(3)},${block.bounds.bottom.toFixed(3)}`}
                      className="pointer-events-none absolute border border-[var(--color-accent-light)] bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)]"
                      style={{
                        left: `${block.bounds.left * 100}%`,
                        top: `${block.bounds.top * 100}%`,
                        width: `${(block.bounds.right - block.bounds.left) * 100}%`,
                        height: `${(block.bounds.bottom - block.bounds.top) * 100}%`,
                      }}
                    />
                  ))}
                </div>
              </section>
            )}

            <ResultSection title="最终会放入提示词的软件上下文" value={result.formattedContext} />
            {result.captureMethod === "nativeText" ? (
              <>
                <ResultSection title="选中文本" value={result.selectedText} />
                <ResultSection title="焦点输入区域内容" value={result.focusedText} />
                <ResultSection title="光标附近内容" value={result.caretContext} />
                <ResultSection title={`可见正文（${result.visibleText.length} 项）`} value={result.visibleText.join("\n")} />
                <ResultSection title={`文档正文（${result.documentText.length} 项）`} value={result.documentText.join("\n")} />
              </>
            ) : (
              <>
                <ResultSection title={`整窗 OCR 文本（${result.ocrText.length} 项）`} value={result.ocrText.join("\n")} />
                <ResultSection
                  title={`OCR 文字框、置信度与归一化坐标（${result.ocrBlocks.length} 项）`}
                  value={result.ocrBlocks.map((block) => (
                    `${(block.confidence * 100).toFixed(1)}% · [${block.bounds.left.toFixed(3)}, ${block.bounds.top.toFixed(3)}, ${block.bounds.right.toFixed(3)}, ${block.bounds.bottom.toFixed(3)}] · ${block.text}`
                  )).join("\n")}
                />
              </>
            )}
          </div>
        )}
      </main>
    </div>
  );
}
