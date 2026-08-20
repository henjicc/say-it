import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Copy, CornerDownLeft, Mic, Pin, RefreshCw, SendHorizontal, X } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Button } from "@/components/ui/Button";
import { CMD, cmd, on } from "@/lib/tauri";
import "./index.css";
import "./assistant.css";

interface Answer {
  action?: "translateSpeech" | "editSelection" | "ask" | null;
  text: string;
  reasoning: string;
  sourceText: string;
  error?: string | null;
  canInsert: boolean;
  streaming: boolean;
  pinned: boolean;
}

interface VoiceInputState {
  active: boolean;
  text: string;
}

interface VoiceWaveform {
  active: boolean;
  level: number;
  peaks: number[];
}

const EMPTY_ANSWER: Answer = {
  text: "",
  reasoning: "",
  sourceText: "",
  canInsert: false,
  streaming: false,
  pinned: false,
};

function SafeMarkdown({ text }: { text: string }) {
  return <div className="assistant-markdown text-sm leading-6 text-[var(--color-fg)]">
    <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
  </div>;
}

function VoiceWave({ values }: { values: number[] }) {
  const samples = values.length > 0 ? values : Array.from({ length: 30 }, () => 0.025);
  return <div className="assistant-voice-wave" role="img" aria-label="正在接收语音">
    {samples.map((value, index) => <span
      // 波形是短生命周期的实时采样序列，序号就是稳定位置。
      key={index}
      style={{ height: `${Math.max(2, Math.min(22, value * 54))}px` }}
    />)}
  </div>;
}

export function AssistantAnswerApp() {
  const [answer, setAnswer] = useState<Answer>(EMPTY_ANSWER);
  const [message, setMessage] = useState("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [voiceActive, setVoiceActive] = useState(false);
  const [waveform, setWaveform] = useState<number[]>([]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const register = async <T,>(event: string, handler: (payload: T) => void) => {
      const unlisten = await on<T>(event, handler);
      if (disposed) unlisten(); else unlisteners.push(unlisten);
    };
    void cmd<Answer>(CMD.getAssistantAnswer).then((value) => { if (!disposed) setAnswer(value); });
    void register<Answer>("assistant-answer-changed", setAnswer);
    void register<VoiceInputState>("assistant-answer-voice-input", (value) => {
      setVoiceActive(value.active);
      if (value.active) {
        if (value.text) setDraft(value.text);
      } else {
        setDraft("");
        setWaveform([]);
      }
    });
    void register<VoiceWaveform>("assistant-answer-waveform", (value) => {
      if (!value.active) return;
      setWaveform((current) => [...current, ...value.peaks].slice(-36));
    });
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") void cmd(CMD.closeAssistantAnswer);
    };
    window.addEventListener("keydown", keydown);
    return () => {
      disposed = true;
      for (const unlisten of unlisteners) unlisten();
      window.removeEventListener("keydown", keydown);
    };
  }, []);

  async function copy() {
    await navigator.clipboard.writeText(answer.text);
    setMessage("已复制");
  }

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

  async function togglePinned() {
    try {
      const next = await cmd<Answer>(CMD.setAssistantAnswerPinned, { pinned: !answer.pinned });
      setAnswer(next);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function startVoiceInput() {
    if (voiceActive || answer.streaming || busy) return;
    setMessage("");
    setDraft("");
    setWaveform([]);
    try {
      await cmd(CMD.startAssistantFollowUpVoice);
      setVoiceActive(true);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function sendFollowUp() {
    if (answer.streaming || busy) return;
    setMessage("");
    if (voiceActive) {
      setVoiceActive(false);
      setWaveform([]);
      try {
        await cmd(CMD.stopAssistantFollowUpVoice);
      } catch (error) {
        setMessage(String(error));
      }
      return;
    }
    const prompt = draft.trim();
    if (!prompt) return;
    setBusy(true);
    setDraft("");
    try {
      await cmd(CMD.continueAssistantAnswer, { prompt });
    } catch (error) {
      const text = String(error);
      if (!text.includes("已取消")) {
        setMessage(text);
        setDraft(prompt);
      }
    } finally {
      setBusy(false);
    }
  }

  return <div className="assistant-answer-window flex flex-col overflow-hidden text-[var(--color-fg)]">
    <header data-tauri-drag-region className="flex h-12 flex-none items-center border-b border-[var(--color-line)] px-4">
      <strong data-tauri-drag-region className="text-sm">智能助手</strong>
      <button
        type="button"
        className={`assistant-title-button ml-auto ${answer.pinned ? "is-active" : ""}`}
        aria-label={answer.pinned ? "取消置顶" : "置顶窗口"}
        aria-pressed={answer.pinned}
        title={answer.pinned ? "取消置顶" : "置顶窗口"}
        onClick={() => void togglePinned()}
      >
        <Pin className="h-4 w-4" />
      </button>
      <button type="button" className="assistant-title-button" aria-label="关闭" onClick={() => void cmd(CMD.closeAssistantAnswer)}>
        <X className="h-4 w-4" />
      </button>
    </header>
    <main className="min-h-0 flex-1 overflow-y-auto p-5">
      {answer.sourceText && <blockquote className="mb-4 line-clamp-3 border-l-2 border-[var(--color-accent)] pl-3 text-xs text-[var(--color-fg-subtle)]">{answer.sourceText}</blockquote>}
      {answer.reasoning && <details className="assistant-reasoning mb-4" open={answer.streaming}>
        <summary>思考过程{answer.streaming ? "（进行中）" : ""}</summary>
        <div className="mt-2 whitespace-pre-wrap break-words">{answer.reasoning}</div>
      </details>}
      {answer.error
        ? <p className="text-sm text-[var(--color-err)]">{answer.error}</p>
        : answer.text
          ? <SafeMarkdown text={answer.text} />
          : <p className="text-sm text-[var(--color-fg-subtle)]">{answer.streaming ? "正在生成回答…" : "正在等待回答…"}</p>}
    </main>
    <footer className="assistant-footer flex-none border-t border-[var(--color-line)]">
      <div className="assistant-actions">
        <Button size="sm" onClick={() => void copy()} disabled={!answer.text || answer.streaming}><Copy className="h-4 w-4" />复制</Button>
        <Button size="sm" onClick={() => void regenerate()} disabled={busy || answer.streaming || voiceActive}><RefreshCw className="h-4 w-4" />重新生成</Button>
        {answer.canInsert && <Button size="sm" variant="primary" onClick={() => void cmd(CMD.insertAssistantAnswer)} disabled={answer.streaming || voiceActive}><CornerDownLeft className="h-4 w-4" />插入当前位置</Button>}
        <span role="status" className="ml-auto truncate text-xs text-[var(--color-fg-subtle)]">{message}</span>
      </div>
      <div className={`assistant-composer ${voiceActive ? "is-listening" : ""}`}>
        <button
          type="button"
          className="assistant-composer-button"
          aria-label={voiceActive ? "正在语音输入" : "开始语音输入"}
          aria-pressed={voiceActive}
          title="语音输入"
          disabled={answer.streaming || busy}
          onClick={() => void startVoiceInput()}
        >
          <Mic className="h-4 w-4" />
        </button>
        <div className="assistant-composer-input">
          {voiceActive
            ? <VoiceWave values={waveform} />
            : <textarea
              value={draft}
              rows={1}
              maxLength={8000}
              aria-label="继续追问"
              placeholder="继续追问…"
              disabled={answer.streaming || busy}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void sendFollowUp();
                }
              }}
            />}
        </div>
        <button
          type="button"
          className="assistant-send-button"
          aria-label={voiceActive ? "结束语音并发送" : "发送追问"}
          title={voiceActive ? "结束语音并发送" : "发送追问"}
          disabled={answer.streaming || busy || (!voiceActive && !draft.trim())}
          onClick={() => void sendFollowUp()}
        >
          <SendHorizontal className="h-4 w-4" />
        </button>
      </div>
    </footer>
  </div>;
}

const root = document.getElementById("assistant-root");
if (root) createRoot(root).render(<StrictMode><AssistantAnswerApp /></StrictMode>);
