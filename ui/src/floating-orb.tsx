import { StrictMode, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AlertTriangle, Check, Clipboard, LoaderCircle, Mic } from "lucide-react";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { CMD, EVT, cmd } from "@/lib/tauri";
import { playCueKind } from "@/lib/cues";
import {
  floatingOrbLabel,
  floatingOrbWaveScale,
  shouldStartOrbDrag,
  type OrbPhase,
} from "@/floating-orb/interaction";
import "@/floating-orb.css";

interface OrbStatePayload {
  phase?: OrbPhase;
  message?: string | null;
}

interface WaveformPayload {
  active?: boolean;
  level?: number;
  peaks?: number[];
}

const EMPTY_WAVEFORM = { level: 0, peaks: [] as number[] };
const WAVE_BAR_COUNT = 5;

function clampLevel(value: unknown): number {
  return Math.max(0, Math.min(1, Number(value) || 0));
}

function FloatingOrbApp() {
  const [phase, setPhase] = useState<OrbPhase>("idle");
  const [message, setMessage] = useState("");
  const [waveform, setWaveform] = useState(EMPTY_WAVEFORM);
  const pointer = useRef({ id: -1, x: 0, y: 0, dragging: false });

  useTauriEvent<OrbStatePayload>("floating-orb-state", (payload) => {
    const next = payload.phase || "idle";
    setPhase(next);
    setMessage(payload.message || "");
    if (next !== "recording") {
      setWaveform(EMPTY_WAVEFORM);
    }
  });

  useTauriEvent<WaveformPayload>(EVT.indicatorWaveform, (payload) => {
    if (!payload.active) {
      setWaveform(EMPTY_WAVEFORM);
      return;
    }
    const peaks = Array.isArray(payload.peaks)
      ? payload.peaks.map((value) => floatingOrbWaveScale(clampLevel(value))).slice(-WAVE_BAR_COUNT)
      : [];
    setWaveform({ level: floatingOrbWaveScale(clampLevel(payload.level)), peaks });
  });

  useTauriEvent<{ which?: "start" | "end"; kind?: string }>(
    EVT.indicatorPlayCue,
    (payload) => {
      if (payload.kind) playCueKind(payload.kind, payload.which || "start");
    },
  );

  const onPointerDown = (event: PointerEvent<HTMLButtonElement>) => {
    if (phase !== "idle" || event.button !== 0) return;
    pointer.current = {
      id: event.pointerId,
      x: event.screenX,
      y: event.screenY,
      dragging: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: PointerEvent<HTMLButtonElement>) => {
    const current = pointer.current;
    if (phase !== "idle" || current.id !== event.pointerId || current.dragging) return;
    if (!shouldStartOrbDrag(event.screenX - current.x, event.screenY - current.y)) return;
    current.dragging = true;
    void getCurrentWindow().startDragging();
  };

  const onPointerUp = (event: PointerEvent<HTMLButtonElement>) => {
    const current = pointer.current;
    if (current.id !== event.pointerId) return;
    pointer.current = { id: -1, x: 0, y: 0, dragging: false };
    if (current.dragging || phase !== "idle") return;
    void cmd(CMD.floatingOrbActivate).catch(() => undefined);
  };

  const stop = () => {
    if (phase === "recording") void cmd(CMD.floatingOrbStop).catch(() => undefined);
  };

  const label = phase === "idle"
    ? "开始悬浮球语音输入；拖动可调整位置"
    : phase === "busy"
      ? "当前有其他音频任务"
      : floatingOrbLabel(phase, message);
  const waveformBars = Array.from(
    { length: WAVE_BAR_COUNT },
    (_, index) => waveform.peaks[index] ?? waveform.level,
  );
  const loading = phase === "moving" || phase === "processing" || phase === "smartProcessing";

  return (
    <button
      type="button"
      className={`floating-orb ${phase}`}
      style={{ "--wave-level": waveform.level } as CSSProperties}
      disabled={phase !== "idle" && phase !== "recording"}
      aria-label={phase === "recording" ? "点击停止识别" : label}
      title={phase === "idle" ? "点击开始语音输入，拖动调整位置" : label}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        pointer.current = { id: -1, x: 0, y: 0, dragging: false };
      }}
      onClick={stop}
    >
      {phase === "recording" ? (
        <span className="orb-waveform" aria-hidden>
          {waveformBars.map((value, index) => (
            <span
              key={index}
              className="orb-wave-bar"
              style={{ "--bar-scale": Math.max(0.18, value) } as CSSProperties}
            />
          ))}
        </span>
      ) : loading || phase === "busy" ? (
        <LoaderCircle className="orb-state-icon orb-spinner" aria-hidden />
      ) : phase === "success" ? (
        <Check className="orb-state-icon" aria-hidden />
      ) : phase === "fallback" ? (
        <Clipboard className="orb-state-icon" aria-hidden />
      ) : phase === "error" ? (
        <AlertTriangle className="orb-state-icon" aria-hidden />
      ) : (
        <Mic className="orb-state-icon" aria-hidden />
      )}
      <span className="orb-sr-only" aria-live="polite">{label}</span>
    </button>
  );
}

createRoot(document.getElementById("floating-orb-root")!).render(
  <StrictMode>
    <FloatingOrbApp />
  </StrictMode>,
);
