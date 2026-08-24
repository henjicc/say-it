import { StrictMode, useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { createRoot } from "react-dom/client";
import { AlertTriangle, Check, Clipboard, CornerDownLeft, LoaderCircle, Mic, X } from "lucide-react";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { CMD, EVT, cmd, type AppSnapshot, type FloatingOrbSettings } from "@/lib/tauri";
import { playCueKind } from "@/lib/cues";
import { applySystemGlassToDocument, applyThemeToDocument, type AccentTheme } from "@/store/useThemeStore";
import {
  DEFAULT_FLOATING_ORB_APPEARANCE,
  floatingOrbClickAction,
  floatingOrbLabel,
  floatingOrbWaveScale,
  normalizeFloatingOrbAppearance,
  shouldStartOrbDrag,
  type FloatingOrbAppearance,
  type OrbPhase,
} from "@/floating-orb/interaction";
import "@/index.css";
import "@/floating-orb.css";

interface OrbStatePayload {
  phase?: OrbPhase;
  message?: string | null;
  transient?: boolean;
  canSubmit?: boolean;
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
  const [transient, setTransient] = useState(false);
  const [canSubmit, setCanSubmit] = useState(false);
  const [waveform, setWaveform] = useState(EMPTY_WAVEFORM);
  const [appearance, setAppearance] = useState<FloatingOrbAppearance>(
    DEFAULT_FLOATING_ORB_APPEARANCE,
  );
  const pointer = useRef({ id: -1, x: 0, y: 0, dragging: false });

  useEffect(() => {
    void cmd<AppSnapshot>(CMD.getAppSnapshot)
      .then((snapshot) => applyThemeToDocument(snapshot.settings.theme as Partial<AccentTheme>))
      .catch(() => undefined);
    void cmd<FloatingOrbSettings>(CMD.getFloatingOrbSettings)
      .then((settings) => setAppearance(normalizeFloatingOrbAppearance(settings)))
      .catch(() => undefined);
  }, []);

  useTauriEvent<Partial<AccentTheme>>(EVT.themeChanged, applyThemeToDocument);

  useTauriEvent<Partial<FloatingOrbAppearance>>("floating-orb-config", (payload) => {
    const next = normalizeFloatingOrbAppearance(payload);
    setAppearance(next);
    applySystemGlassToDocument(next);
  });

  useEffect(() => {
    applySystemGlassToDocument(appearance);
  }, [appearance.glassEnabled, appearance.glassTint]);

  useTauriEvent<OrbStatePayload>("floating-orb-state", (payload) => {
    const next = payload.phase || "idle";
    setPhase(next);
    setMessage(payload.message || "");
    setTransient(payload.transient === true);
    setCanSubmit(payload.canSubmit === true);
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
    if (phase !== "idle" || transient || event.button !== 0) return;
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
    if (phase !== "idle" || transient || current.id !== event.pointerId || current.dragging) return;
    if (!shouldStartOrbDrag(event.screenX - current.x, event.screenY - current.y)) return;
    current.dragging = true;
    void cmd(CMD.floatingOrbStartDragging).catch(() => undefined);
  };

  const onPointerUp = (event: PointerEvent<HTMLButtonElement>) => {
    const current = pointer.current;
    if (current.id !== event.pointerId) return;
    pointer.current = { id: -1, x: 0, y: 0, dragging: false };
    if (current.dragging || phase !== "idle") return;
    void cmd(CMD.floatingOrbActivate).catch(() => undefined);
  };

  const activate = () => {
    const action = floatingOrbClickAction(phase, canSubmit);
    if (action === "activate") {
      void cmd(CMD.floatingOrbActivate).catch(() => undefined);
    } else if (action === "stop") {
      void cmd(CMD.floatingOrbStop).catch(() => undefined);
    } else if (action === "submit") {
      void cmd(CMD.floatingOrbSubmitEnter).catch(() => undefined);
    }
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
  const loading = phase === "moving" || phase === "processing" || phase === "smartProcessing" || phase === "submitting";
  const interactive = phase === "idle" || phase === "armed" || phase === "recording" || (phase === "success" && canSubmit);
  const title = phase === "idle"
    ? transient ? label : "点击开始语音输入，拖动调整位置"
    : phase === "success" && canSubmit
      ? "点击发送回车"
      : label;

  return (
    <button
      type="button"
      className={`floating-orb ${phase}${transient ? " transient" : ""}${appearance.glassEnabled ? " glass" : ""}`}
      style={{
        "--orb-opacity": appearance.opacity / 100,
        "--orb-glass-tint": `${appearance.glassTint}%`,
        "--orb-glass-border": `${appearance.glassBorder}%`,
      } as CSSProperties}
      disabled={!interactive}
      aria-label={phase === "recording" ? "点击停止识别" : phase === "success" && canSubmit ? "点击发送回车" : label}
      title={title}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        pointer.current = { id: -1, x: 0, y: 0, dragging: false };
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        if (phase === "recording") {
          void cmd(CMD.floatingOrbCancel).catch(() => undefined);
        } else if (!transient && phase === "idle") {
          void cmd(CMD.showFloatingOrbMenu).catch(() => undefined);
        }
      }}
      onClick={activate}
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
      ) : (
        <span className="orb-icon-shell" aria-hidden>
          {loading || phase === "busy" ? (
            <LoaderCircle key={phase} className="orb-state-icon orb-spinner" />
          ) : phase === "success" ? (
            <>
              <Check className={`orb-state-icon${canSubmit ? " orb-success-check" : ""}`} />
              {canSubmit && <CornerDownLeft className="orb-state-icon orb-submit-icon" />}
            </>
          ) : phase === "submitted" ? (
            <CornerDownLeft className="orb-state-icon" />
          ) : phase === "fallback" ? (
            <Clipboard className="orb-state-icon" />
          ) : phase === "cancelled" ? (
            <X className="orb-state-icon" />
          ) : phase === "error" ? (
            <AlertTriangle className="orb-state-icon" />
          ) : (
            <Mic className="orb-state-icon" />
          )}
        </span>
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
