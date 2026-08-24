import { StrictMode, useRef, useState, type PointerEvent } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AlertTriangle, Check, Clipboard, LoaderCircle, Mic, Square } from "lucide-react";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { CMD, EVT, cmd } from "@/lib/tauri";
import { playCueKind } from "@/lib/cues";
import {
  floatingOrbLabel,
  shouldStartOrbDrag,
  type OrbPhase,
} from "@/floating-orb/interaction";
import "@/floating-orb.css";

interface OrbStatePayload {
  phase?: OrbPhase;
  message?: string | null;
}

function FloatingOrbApp() {
  const [phase, setPhase] = useState<OrbPhase>("idle");
  const [message, setMessage] = useState("");
  const [stopHovered, setStopHovered] = useState(false);
  const pointer = useRef({ id: -1, x: 0, y: 0, dragging: false });

  useTauriEvent<OrbStatePayload>("floating-orb-state", (payload) => {
    const next = payload.phase || "idle";
    setPhase(next);
    setMessage(payload.message || "");
    if (next !== "recording") setStopHovered(false);
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
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    void cmd(CMD.floatingOrbActivate, { reducedMotion }).catch(() => undefined);
  };

  const stop = () => {
    if (phase === "recording") void cmd(CMD.floatingOrbStop).catch(() => undefined);
  };

  if (phase === "idle" || phase === "busy") {
    return (
      <button
        type="button"
        className={`floating-orb ${phase}`}
        aria-label={phase === "busy" ? "当前有其他音频任务" : "开始悬浮球语音输入；拖动可调整位置"}
        title={phase === "busy" ? "当前有其他音频任务" : "点击开始语音输入，拖动调整位置"}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          pointer.current = { id: -1, x: 0, y: 0, dragging: false };
        }}
      >
        {phase === "busy" ? <LoaderCircle aria-hidden /> : <Mic aria-hidden />}
      </button>
    );
  }

  const label = floatingOrbLabel(phase, message, stopHovered);

  const Icon = stopHovered && phase === "recording"
    ? Square
    : phase === "moving" || phase === "processing" || phase === "smartProcessing"
      ? LoaderCircle
      : phase === "success"
        ? Check
        : phase === "fallback"
          ? Clipboard
          : phase === "error"
            ? AlertTriangle
            : Mic;

  return (
    <button
      type="button"
      className={`floating-bubble ${phase}${stopHovered ? " stop-hovered" : ""}`}
      disabled={phase !== "recording"}
      aria-label={phase === "recording" ? "停止识别" : label}
      onPointerEnter={() => phase === "recording" && setStopHovered(true)}
      onPointerLeave={() => setStopHovered(false)}
      onClick={stop}
    >
      <Icon aria-hidden />
      <span>{label}</span>
    </button>
  );
}

createRoot(document.getElementById("floating-orb-root")!).render(
  <StrictMode>
    <FloatingOrbApp />
  </StrictMode>,
);
