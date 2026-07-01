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
}

const LABELS: Record<Exclude<Phase, "hidden">, string> = {
  recording: "正在聆听…",
  processing: "识别中…",
  subtitle: "实时字幕",
};

const MAX_RENDER_CHARS = 20000;
const FRESH_FADE_MAX = 10;
const OVERLAP_SEARCH_MAX = 200;

export function IndicatorApp() {
  const [phase, setPhase] = useState<Phase>("hidden");
  const [mode, setMode] = useState<IndicatorMode>("dictation");
  const [subtitleConfig, setSubtitleConfig] = useState<SubtitleConfig>({});
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

  const applyScroll = () => {
    const content = textContentRef.current;
    const flow = textFlowRef.current;
    if (!content || !flow) return;
    // 单句替换模式框高只留 1 行，但一句话说久了内容仍可能换行溢出；
    // 这里统一按溢出量向上滚动，让最新说的内容始终留在可见行内，不会被锁在框顶而"卡住不更新"。
    const overflow = content.scrollHeight - flow.clientHeight;
    const offset = overflow > 0 ? overflow : 0;
    const next = `translate3d(0, ${-offset}px, 0)`;
    if (next !== lastTransform.current) {
      lastTransform.current = next;
      content.style.transform = next;
    }
  };

  const resetText = () => {
    pendingText.current = "";
    displayedText.current = "";
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
    pendingText.current =
      nextText.length > MAX_RENDER_CHARS
        ? nextText.slice(-MAX_RENDER_CHARS).replace(/^\s+/, "")
        : nextText;
    textElRef.current?.classList.remove("empty");
    if (renderFrame.current) return;
    renderFrame.current = requestAnimationFrame(() => {
      renderFrame.current = 0;
      if (displayedText.current !== pendingText.current) {
        paintText(pendingText.current);
      }
      applyScroll();
    });
  };

  useTauriEvent<{ state?: Phase }>(EVT.indicatorState, (payload) => {
    const next = payload.state || "hidden";
    setPhase(next);
    if (next === "hidden") resetText();
  });

  useTauriEvent<{ text?: string; fade?: boolean }>(EVT.indicatorText, (payload) => {
    payload.fade ? swapText(payload.text || "") : renderText(payload.text || "");
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
        } as React.CSSProperties)
      : undefined;

  return (
    <div
      id="wrap"
      className={mode === "subtitle" ? "subtitle-mode" : "dictation-mode"}
      style={{ display: visible ? "flex" : "none", ...subtitleStyle }}
    >
      <div id="text" ref={textElRef} className="empty">
        <div id="text-flow" ref={textFlowRef}>
          <div id="text-content" ref={textContentRef} />
        </div>
      </div>
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
