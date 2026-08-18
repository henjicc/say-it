import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Copy, CornerDownLeft, RefreshCw, X } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { CMD, cmd, on } from "@/lib/tauri";
import "./index.css";

interface Answer { action?: "translateSpeech" | "editSelection" | "ask" | null; text: string; sourceText: string; error?: string | null; canInsert: boolean }

function SafeMarkdown({ text }: { text: string }) {
  return <div className="space-y-2 text-sm leading-6 text-[var(--color-fg)]">{text.split(/\n{2,}/).map((block, index) => {
    const lines = block.split("\n");
    if (lines.every((line) => /^[-*]\s/.test(line))) return <ul key={index} className="list-disc space-y-1 pl-5">{lines.map((line) => <li key={line}>{line.replace(/^[-*]\s/, "")}</li>)}</ul>;
    if (/^#{1,3}\s/.test(lines[0] || "")) return <h2 key={index} className="text-base font-semibold">{block.replace(/^#{1,3}\s/, "")}</h2>;
    return <p key={index} className="whitespace-pre-wrap break-words">{block}</p>;
  })}</div>;
}

export function AssistantAnswerApp() {
  const [answer, setAnswer] = useState<Answer>({ text: "", sourceText: "", canInsert: false });
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void cmd<Answer>(CMD.getAssistantAnswer).then(setAnswer);
    let unlisten: (() => void) | undefined;
    void on<Answer>("assistant-answer-changed", setAnswer).then((value) => { unlisten = value; });
    const keydown = (event: KeyboardEvent) => { if (event.key === "Escape") void cmd(CMD.closeAssistantAnswer); };
    window.addEventListener("keydown", keydown);
    return () => { unlisten?.(); window.removeEventListener("keydown", keydown); };
  }, []);
  async function copy() { await navigator.clipboard.writeText(answer.text); setMessage("已复制"); }
  async function regenerate() {
    setBusy(true);
    setMessage("");
    try {
      await cmd(CMD.regenerateAssistantAnswer);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  return <div className="flex h-screen flex-col overflow-hidden bg-[var(--color-overlay)] text-[var(--color-fg)]">
    <header data-tauri-drag-region className="flex h-12 flex-none items-center border-b border-[var(--color-line)] px-4"><strong data-tauri-drag-region className="text-sm">语音助手</strong><button type="button" className="ml-auto rounded-[var(--radius-sm)] p-2 hover:bg-[var(--color-surface)]" aria-label="关闭" onClick={() => void cmd(CMD.closeAssistantAnswer)}><X className="h-4 w-4" /></button></header>
    <main className="min-h-0 flex-1 overflow-y-auto p-5">{answer.sourceText && <blockquote className="mb-4 line-clamp-3 border-l-2 border-[var(--color-accent)] pl-3 text-xs text-[var(--color-fg-subtle)]">{answer.sourceText}</blockquote>}{answer.error ? <p className="text-sm text-[var(--color-err)]">{answer.error}</p> : answer.text ? <SafeMarkdown text={answer.text} /> : <p className="text-sm text-[var(--color-fg-subtle)]">正在等待回答…</p>}</main>
    <footer className="flex flex-none items-center gap-2 border-t border-[var(--color-line)] p-3"><Button size="sm" onClick={() => void copy()} disabled={!answer.text}><Copy className="h-4 w-4" />复制</Button><Button size="sm" onClick={() => void regenerate()} disabled={busy}><RefreshCw className="h-4 w-4" />重新生成</Button>{answer.canInsert && <Button size="sm" variant="primary" onClick={() => void cmd(CMD.insertAssistantAnswer)}><CornerDownLeft className="h-4 w-4" />插入当前位置</Button>}<span role="status" className="ml-auto text-xs text-[var(--color-fg-subtle)]">{message}</span></footer>
  </div>;
}

const root = document.getElementById("assistant-root");
if (root) createRoot(root).render(<StrictMode><AssistantAnswerApp /></StrictMode>);
