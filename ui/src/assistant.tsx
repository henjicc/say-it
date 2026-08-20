import { StrictMode, useEffect, useRef, useState } from "react";
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

export const VOICE_WAVE_BAR_COUNT = 28;
const VOICE_WAVE_MAX_AMPLITUDE = 0.82;
const VOICE_WAVE_MIN_SCALE = 0.09;

function normalizeVoiceAmplitude(value: number) {
  if (!Number.isFinite(value) || value <= 0) return 0;
  const db = 20 * Math.log10(Math.max(value, 0.00001));
  if (db <= -48) return 0;
  const normalized = Math.min(1, (db + 48) / 40);
  return Math.min(VOICE_WAVE_MAX_AMPLITUDE, Math.pow(normalized, 1.65) * VOICE_WAVE_MAX_AMPLITUDE);
}

export function buildVoiceWaveTargets(frame: Pick<VoiceWaveform, "level" | "peaks">) {
  const level = normalizeVoiceAmplitude(frame.level);
  const source = frame.peaks.length > 0 ? frame.peaks : [frame.level];
  return Array.from({ length: VOICE_WAVE_BAR_COUNT }, (_, index) => {
    // 固定柱位并左右镜像：每一帧只改变原地高度，不累积历史，也不会产生横向滚动感。
    const distance = Math.abs(index - (VOICE_WAVE_BAR_COUNT - 1) / 2)
      / ((VOICE_WAVE_BAR_COUNT - 1) / 2);
    const sourceIndex = Math.round(distance * (source.length - 1));
    const local = normalizeVoiceAmplitude(source[sourceIndex] ?? frame.level);
    const envelope = 1 - distance * 0.38;
    return Math.min(VOICE_WAVE_MAX_AMPLITUDE, (level * 0.68 + local * 0.32) * envelope);
  });
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

function VoiceWave({ frame }: { frame: VoiceWaveform }) {
  const barsRef = useRef<Array<HTMLSpanElement | null>>([]);
  const targetsRef = useRef<number[]>(Array(VOICE_WAVE_BAR_COUNT).fill(0));

  useEffect(() => {
    targetsRef.current = buildVoiceWaveTargets(frame);
  }, [frame]);

  useEffect(() => {
    const current = Array(VOICE_WAVE_BAR_COUNT).fill(0) as number[];
    let animationFrame = 0;
    let previousTime = performance.now();
    const animate = (time: number) => {
      const elapsed = Math.min(50, Math.max(1, time - previousTime));
      previousTime = time;
      for (let index = 0; index < VOICE_WAVE_BAR_COUNT; index += 1) {
        const target = targetsRef.current[index] ?? 0;
        const timeConstant = target > current[index] ? 58 : 135;
        const blend = 1 - Math.exp(-elapsed / timeConstant);
        current[index] += (target - current[index]) * blend;
        const scale = VOICE_WAVE_MIN_SCALE + current[index] * (1 - VOICE_WAVE_MIN_SCALE);
        const bar = barsRef.current[index];
        if (bar && Math.abs(Number(bar.dataset.scale || 0) - scale) > 0.002) {
          bar.dataset.scale = String(scale);
          bar.style.transform = `scaleY(${scale})`;
        }
      }
      animationFrame = requestAnimationFrame(animate);
    };
    animationFrame = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animationFrame);
  }, []);

  return <div className="assistant-voice-wave" role="img" aria-label="正在接收语音">
    {Array.from({ length: VOICE_WAVE_BAR_COUNT }, (_, index) => <span
      key={index}
      ref={(node) => { barsRef.current[index] = node; }}
    />)}
  </div>;
}

export function AssistantAnswerApp() {
  const [answer, setAnswer] = useState<Answer>(EMPTY_ANSWER);
  const [message, setMessage] = useState("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [voiceActive, setVoiceActive] = useState(false);
  const [waveform, setWaveform] = useState<VoiceWaveform>({ active: false, level: 0, peaks: [] });

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
        setWaveform({ active: false, level: 0, peaks: [] });
      }
    });
    void register<VoiceWaveform>("assistant-answer-waveform", (value) => {
      if (!value.active) return;
      setWaveform(value);
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
    setWaveform({ active: true, level: 0, peaks: [] });
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
      setWaveform({ active: false, level: 0, peaks: [] });
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
            ? <VoiceWave frame={waveform} />
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
