import { useEffect, useRef, useState } from "react";
import { EVT, emitEvent } from "@/lib/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";

type Phase = "hidden" | "recording" | "processing" | "subtitle";
type IndicatorMode = "dictation" | "subtitle";

interface SubtitleConfig {
  displayMode?: "scroll" | "replace";
  fontFamily?: string;
  fontSize?: number;
  lineCount?: number;
  textColor?: string;
  backgroundColor?: string;
  rounded?: number;
  width?: number;
  motionEnabled?: boolean;
  motionDurationMs?: number;
  motionEasing?: string;
  fadeEnabled?: boolean;
  fadeDurationMs?: number;
  fadeEasing?: string;
}

const LABELS: Record<Exclude<Phase, "hidden">, string> = {
  recording: "正在聆听…",
  processing: "识别中…",
  subtitle: "实时字幕",
};

const MAX_RENDER_CHARS = 20000;
const FRESH_FADE_MAX = 10;
const OVERLAP_SEARCH_MAX = 200;
const WAVE_BAR_COUNT = 24;

export function IndicatorApp() {
  const [phase, setPhase] = useState<Phase>("hidden");
  const [mode, setMode] = useState<IndicatorMode>("dictation");
  const [subtitleConfig, setSubtitleConfig] = useState<SubtitleConfig>({});
  const [waveform, setWaveform] = useState({ active: false, level: 0, peaks: [] as number[] });
  const [hasText, setHasText] = useState(false);
  const textElRef = useRef<HTMLDivElement>(null);
  const textFlowRef = useRef<HTMLDivElement>(null);
  const textContentRef = useRef<HTMLDivElement>(null);

  const pendingText = useRef("");
  const displayedText = useRef("");
  const renderFrame = useRef(0);
  const lastTransform = useRef("");

  const paintText = (next: string) => {
    const content = textContentRef.current;
    if (!content) return;
    const prevText = displayedText.current;
    const min = Math.min(prevText.length, next.length);
    let prefix = 0;
    while (prefix < min && prevText[prefix] === next[prefix]) prefix += 1;
    let overlap = prefix;
    const searchFloor = Math.max(prefix, min - OVERLAP_SEARCH_MAX);
    for (let k = min; k > searchFloor; k -= 1) {
      if (prevText.endsWith(next.slice(0, k))) {
        overlap = k;
        break;
      }
    }
    const stable = next.slice(0, overlap);
    const fresh = next.slice(overlap);
    content.textContent = "";
    if (stable) content.appendChild(document.createTextNode(stable));
    if (fresh) {
      const span = document.createElement("span");
      if (fresh.length <= FRESH_FADE_MAX) span.className = "fresh";
      span.textContent = fresh;
      content.appendChild(span);
    }
    displayedText.current = next;
  };

  const applyScroll = (skipAnimation = false) => {
    const content = textContentRef.current;
    const flow = textFlowRef.current;
    if (!content || !flow) return;
    let next: string;
    if (isReplaceMode) {
      // 单句替换模式强制单行不换行，靠左右平移展示：句子短时居中，
      // 超出框宽后贴右边显示最新内容、旧内容向左流出，避免像滚动模式那样整句被上下顶掉。
      const overflow = content.scrollWidth - flow.clientWidth;
      const offset = overflow > 0 ? -overflow : (flow.clientWidth - content.scrollWidth) / 2;
      next = `translate3d(${offset}px, 0, 0)`;
    } else {
      // 滚动累积模式框高只留固定行数，但内容可能超出；
      // 这里统一按溢出量向上滚动，让最新说的内容始终留在可见区域内，不会被锁在框顶而"卡住不更新"。
      const overflow = content.scrollHeight - flow.clientHeight;
      const offset = overflow > 0 ? overflow : 0;
      next = `translate3d(0, ${-offset}px, 0)`;
    }
    if (next !== lastTransform.current) {
      lastTransform.current = next;
      if (skipAnimation) {
        // 从空白刚出现第一个词时，直接落位到目标位置（如居中），不要让 CSS transition
        // 把它从上一次残留的 transform（左对齐的 0,0）动画着"飞"过去。
        const prevTransition = content.style.transition;
        content.style.transition = "none";
        content.style.transform = next;
        void content.offsetWidth;
        content.style.transition = prevTransition;
      } else {
        content.style.transform = next;
      }
    }
  };

  const resetText = () => {
    pendingText.current = "";
    displayedText.current = "";
    setHasText(false);
    if (textContentRef.current) {
      textContentRef.current.textContent = "";
      textContentRef.current.classList.remove("swap-in");
      lastTransform.current = "translate3d(0, 0px, 0)";
      textContentRef.current.style.transform = lastTransform.current;
    }
    textElRef.current?.classList.add("empty");
  };

  const swapText = (nextText: string) => {
    const content = textContentRef.current;
    displayedText.current = "";
    if (content) {
      content.textContent = "";
      content.classList.remove("swap-in");
      void content.offsetWidth;
      content.classList.add("swap-in");
    }
    renderText(nextText);
  };

  const renderText = (nextText: string) => {
    if (!nextText) {
      if (renderFrame.current) {
        cancelAnimationFrame(renderFrame.current);
        renderFrame.current = 0;
      }
      resetText();
      return;
    }
    setHasText(true);
    pendingText.current =
      nextText.length > MAX_RENDER_CHARS
        ? nextText.slice(-MAX_RENDER_CHARS).replace(/^\s+/, "")
        : nextText;
    textElRef.current?.classList.remove("empty");
    if (renderFrame.current) return;
    renderFrame.current = requestAnimationFrame(() => {
      renderFrame.current = 0;
      const isFirstPaint = !displayedText.current && !!pendingText.current;
      if (displayedText.current !== pendingText.current) {
        paintText(pendingText.current);
      }
      applyScroll(isFirstPaint);
    });
  };

  useTauriEvent<{ state?: Phase }>(EVT.indicatorState, (payload) => {
    const next = payload.state || "hidden";
    setPhase(next);
    if (next === "hidden") {
      resetText();
      setWaveform({ active: false, level: 0, peaks: [] });
    }
  });

  useTauriEvent<{ text?: string; fade?: boolean }>(EVT.indicatorText, (payload) => {
    payload.fade ? swapText(payload.text || "") : renderText(payload.text || "");
  });

  useTauriEvent<{ active?: boolean; level?: number; peaks?: number[] }>(EVT.indicatorWaveform, (payload) => {
    const active = !!payload.active;
    const level = Math.max(0, Math.min(1, Number(payload.level) || 0));
    const peaks = Array.isArray(payload.peaks)
      ? payload.peaks.map((value) => Math.max(0, Math.min(1, Number(value) || 0)))
      : [];
    setWaveform((prev) => ({
      active,
      level,
      peaks: active ? [...prev.peaks, ...peaks].slice(-WAVE_BAR_COUNT) : [],
    }));
  });

  useTauriEvent<{ mode?: IndicatorMode; subtitle?: SubtitleConfig }>(EVT.indicatorConfig, (payload) => {
    setMode(payload.mode || "dictation");
    if (payload.subtitle) setSubtitleConfig(payload.subtitle);
  });

  useEffect(() => {
    return () => {
      if (renderFrame.current) cancelAnimationFrame(renderFrame.current);
    };
  }, []);

  useEffect(() => {
    const isMod = (code: string) =>
      code.startsWith("Control") ||
      code.startsWith("Shift") ||
      code.startsWith("Alt") ||
      code.startsWith("Meta");
    const onKeydown = (event: KeyboardEvent) => {
      if (isMod(event.code)) return;
      emitEvent(EVT.indicatorKeydown, {
        code: event.code,
        ctrlKey: event.ctrlKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        metaKey: event.metaKey,
      });
    };
    const onKeyup = (event: KeyboardEvent) => {
      if (isMod(event.code)) return;
      emitEvent(EVT.indicatorKeyup, { code: event.code });
    };
    window.addEventListener("keydown", onKeydown, true);
    window.addEventListener("keyup", onKeyup, true);
    return () => {
      window.removeEventListener("keydown", onKeydown, true);
      window.removeEventListener("keyup", onKeyup, true);
    };
  }, []);

  const pillPhase = phase === "hidden" ? "recording" : phase;
  const visible = phase !== "hidden";
  const isReplaceMode = mode === "subtitle" && subtitleConfig.displayMode === "replace";
  const isNoMotion = mode === "subtitle" && subtitleConfig.motionEnabled === false;
  const isNoFade = mode === "subtitle" && subtitleConfig.fadeEnabled === false;
  const showWaveform = mode === "dictation" && phase === "recording" && waveform.active;
  const showProcessingPanel = mode === "dictation" && phase === "processing" && !hasText;
  const waveformBars = Array.from({ length: WAVE_BAR_COUNT }, (_, index) => waveform.peaks[index] ?? 0);
  const subtitleStyle =
    mode === "subtitle"
      ? ({
          "--subtitle-font-size": `${subtitleConfig.fontSize || 28}px`,
          "--subtitle-line-height": `${Math.round((subtitleConfig.fontSize || 28) * 1.38)}px`,
          "--subtitle-lines": subtitleConfig.lineCount || 2,
          "--subtitle-width": `${subtitleConfig.width || 880}px`,
          "--subtitle-text": subtitleConfig.textColor || "#fff",
          "--subtitle-bg": subtitleConfig.backgroundColor || "rgba(5, 7, 10, 0.72)",
          "--subtitle-radius": `${subtitleConfig.rounded ?? 18}px`,
          "--subtitle-font": subtitleConfig.fontFamily || "Microsoft YaHei",
          "--subtitle-motion-duration": `${subtitleConfig.motionDurationMs ?? 120}ms`,
          "--subtitle-motion-easing": subtitleConfig.motionEasing || "ease-out",
          "--subtitle-fade-duration": `${subtitleConfig.fadeDurationMs ?? 180}ms`,
          "--subtitle-fade-easing": subtitleConfig.fadeEasing || "ease-out",
        } as React.CSSProperties)
      : undefined;

  return (
    <div
      id="wrap"
      className={
        mode === "subtitle"
          ? `subtitle-mode${isReplaceMode ? " subtitle-replace" : ""}${isNoMotion ? " no-motion" : ""}${isNoFade ? " no-fade" : ""}`
          : "dictation-mode"
      }
      style={{ display: visible ? "flex" : "none", ...subtitleStyle }}
    >
      <div id="text" ref={textElRef} className="empty">
        <div id="text-flow" ref={textFlowRef}>
          <div id="text-content" ref={textContentRef} />
        </div>
      </div>
      {(showWaveform || showProcessingPanel) && (
        <div
          id="signal-panel"
          className={showProcessingPanel ? "processing" : "recording"}
          style={{ "--wave-level": waveform.level } as React.CSSProperties}
        >
          {showWaveform ? (
            <div className="wave-bars" aria-hidden="true">
              {waveformBars.map((value, index) => (
                <span
                  key={index}
                  className="wave-bar"
                  style={{ "--bar-height": `${Math.max(8, 8 + value * 34)}px` } as React.CSSProperties}
                />
              ))}
            </div>
          ) : (
            <div className="loader-dots" aria-hidden="true">
              <span className="loader-dot" />
              <span className="loader-dot" />
              <span className="loader-dot" />
            </div>
          )}
        </div>
      )}
      {pillPhase !== "subtitle" && (
        <div className={`pill ${pillPhase}`} id="pill">
          <span className="dot" />
          <span className="label" id="label">
            {LABELS[pillPhase]}
          </span>
        </div>
      )}
    </div>
  );
}
