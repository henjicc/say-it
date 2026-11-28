import { useEffect, useRef, useState } from "react";
import { ArrowUp } from "lucide-react";
import {
  beginShortcutCapture,
  setMainShortcut,
  setPressHoldMode,
  shortcutLabel,
  type ShortcutCombo,
} from "@/features/dictation/hotkeys";
import { cn } from "@/lib/cn";
import { isMacOS } from "@/lib/platform";
import { useDictationStore } from "@/store/useDictationStore";

interface KeyPreset {
  id: string;
  keyCode: string;
  cap: string;
  isCaps?: boolean;
}

const KEY_PRESETS: readonly KeyPreset[] = isMacOS
  ? [
      { id: "caps", keyCode: "CapsLock", cap: "CapsLock", isCaps: true },
      { id: "fn", keyCode: "Fn", cap: "Fn" },
      { id: "f8", keyCode: "F8", cap: "F8" },
    ]
  : [
      { id: "caps", keyCode: "CapsLock", cap: "CapsLock", isCaps: true },
      { id: "ralt", keyCode: "AltRight", cap: "右 Alt" },
      { id: "f8", keyCode: "F8", cap: "F8" },
    ];

const EMPTY_MODS = { ctrl: false, shift: false, alt: false, meta: false };
const DEMO_WAVE_HEIGHTS = [8, 14, 16, 10, 15, 9];

function isBareKey(shortcut: ShortcutCombo, code: string) {
  return (
    shortcut.keyCode === code
    && !shortcut.ctrl
    && !shortcut.shift
    && !shortcut.alt
    && !shortcut.meta
  );
}

/**
 * 第 4 步 · 快捷键设置 + 逼真键帽与清亮听写胶囊动态演示。
 * 默认 CapsLock 单击（pressHoldMode = false）；切换预设、录制自定义按键或切换触发方式时舞台实时联动。
 */
export function TriggerStep({ error }: { error: string }) {
  const shortcut = useDictationStore((state) => state.shortcut);
  const pressHoldMode = useDictationStore((state) => state.pressHoldMode);
  const [capturing, setCapturing] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [demoPhase, setDemoPhase] = useState<0 | 1 | 2 | 3>(1);
  const cancelRef = useRef<(() => void) | null>(null);

  useEffect(() => () => cancelRef.current?.(), []);

  useEffect(() => {
    const reduced = typeof window !== "undefined"
      && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      setDemoPhase(2);
      return;
    }
    const timer = window.setInterval(() => {
      setDemoPhase((prev) => ((prev + 1) % 4) as 0 | 1 | 2 | 3);
    }, 1550);
    return () => window.clearInterval(timer);
  }, []);

  const activePreset = KEY_PRESETS.find((preset) => isBareKey(shortcut, preset.keyCode));
  const capText = activePreset?.cap || shortcutLabel(shortcut) || "CapsLock";
  const isCaps = activePreset?.isCaps || isBareKey(shortcut, "CapsLock");

  const applyPreset = async (preset: KeyPreset) => {
    cancelRef.current?.();
    setSaveError("");
    setDemoPhase(1);
    try {
      await setMainShortcut({ keyCode: preset.keyCode, ...EMPTY_MODS });
    } catch (reason) {
      setSaveError(String(reason));
    }
  };

  const toggleCapture = () => {
    setSaveError("");
    if (capturing) {
      cancelRef.current?.();
      return;
    }
    setCapturing(true);
    cancelRef.current = beginShortcutCapture(
      async (recorded) => {
        cancelRef.current = null;
        setCapturing(false);
        setDemoPhase(1);
        try {
          await setMainShortcut(recorded);
        } catch (reason) {
          setSaveError(String(reason));
        }
      },
      () => {
        cancelRef.current = null;
        setCapturing(false);
      },
    );
  };

  const applyTriggerMode = async (hold: boolean) => {
    setSaveError("");
    setDemoPhase(1);
    await setPressHoldMode(hold);
  };

  const isPressed = pressHoldMode ? (demoPhase === 1 || demoPhase === 2) : demoPhase === 1;
  const isLit = demoPhase === 1 || demoPhase === 2;
  const capsuleVisible = demoPhase === 1 || demoPhase === 2;

  const step1Text = `${pressHoldMode ? "按住" : "按一下"} ${capText}`;
  const step2Text = pressHoldMode ? "说话，松开结束" : "说话，再按一下结束";

  return (
    <div className="space-y-5" aria-label="设置唤起快捷键">
      <div className="space-y-1.5 text-center">
        <h1 className="text-[24px] font-semibold tracking-tight text-[var(--color-fg)]">设置你的唤起快捷键</h1>
        <p className="text-[14px] text-[var(--color-fg-muted)]">
          默认单击 <span className="font-medium text-[var(--color-fg)]">CapsLock</span> 即可开始说话，也可随时录成其他键。
        </p>
      </div>

      {/* 键帽 + 新版清亮听写胶囊 + 模拟输入框联动舞台 */}
      <div className="space-y-4 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-5 shadow-[var(--shadow-raise)]">
        <div
          role="img"
          aria-label={`${step1Text}，${step2Text}，文字直接输入`}
          className="relative flex flex-col items-center gap-4 overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)] p-5"
        >
          {/* 模拟目标软件输入框 */}
          <div className="flex h-10 w-full max-w-[440px] items-center justify-between gap-2 rounded-[var(--radius-lg)] border border-[var(--color-line-strong)] bg-[var(--color-surface)] px-3.5 text-[13px]">
            <div className="truncate">
              {demoPhase === 0 || demoPhase === 1 ? (
                <span className="text-[var(--color-fg-faint)]">光标所在输入框…</span>
              ) : demoPhase === 2 ? (
                <span className="text-[var(--color-fg-muted)]">下午三点开会，记得带…</span>
              ) : (
                <span className="text-[var(--color-fg)]">下午三点开会，记得带周报。</span>
              )}
              <span className="ob-demo-caret" aria-hidden />
            </div>
            <span
              aria-hidden
              className={cn(
                "flex h-6 w-6 shrink-0 items-center justify-center rounded-full transition-colors",
                demoPhase === 3
                  ? "bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)]"
                  : "bg-[var(--color-surface-hover)] text-[var(--color-fg-faint)]",
              )}
            >
              <ArrowUp className="h-3.5 w-3.5" strokeWidth={2.4} />
            </span>
          </div>

          {/* 逼真动态键帽 + 新版清亮听写胶囊同框联动 */}
          <div className="flex flex-wrap items-center justify-center gap-6 py-1" aria-hidden>
            <div className={cn("ob-keycap", isPressed && "is-pressed", isLit && "is-lit")}>
              {isCaps && <span className="ob-keycap-led" />}
              <span>{capText}</span>
            </div>

            <div
              className="flex h-9 items-center gap-2.5 rounded-full border border-[var(--color-line)] bg-[var(--color-overlay)] px-3.5 shadow-[var(--shadow-popover)] transition-all duration-200"
              style={{
                opacity: capsuleVisible ? 1 : 0,
                transform: capsuleVisible ? "translateY(0) scale(1)" : "translateY(6px) scale(0.96)",
              }}
            >
              <span className="h-2 w-2 rounded-full bg-[var(--color-accent)]" />
              <div className="ob-demo-wave flex h-4 items-center gap-[3px]">
                {DEMO_WAVE_HEIGHTS.map((height, index) => (
                  <i
                    key={index}
                    style={{ height: `${height}px`, animationDelay: `${index * 110}ms` }}
                  />
                ))}
              </div>
              <span className="text-xs font-medium text-[var(--color-fg-muted)]">聆听中</span>
            </div>
          </div>
        </div>

        {/* 同步 3 步说明条 */}
        <div className="flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-xs">
          {[step1Text, step2Text, "文字直接输入"].map((label, index) => {
            const active = demoPhase === index + 1;
            return (
              <span
                key={index}
                className={cn(
                  "flex items-center gap-1.5 transition-colors",
                  active ? "font-semibold text-[var(--color-accent-text)]" : "text-[var(--color-fg-muted)]",
                )}
              >
                <b
                  className={cn(
                    "inline-flex h-4 w-4 items-center justify-center rounded-full text-[10px]",
                    active
                      ? "bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)]"
                      : "bg-[var(--color-surface-hover)] text-[var(--color-fg-muted)]",
                  )}
                >
                  {index + 1}
                </b>
                {label}
              </span>
            );
          })}
        </div>
      </div>

      {/* 快捷键预设 + 自定义录制 + 触发方式 */}
      <div className="flex flex-col gap-3.5 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-raise)]">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span className="text-[13px] font-medium text-[var(--color-fg)]">选择或自定义按键</span>
          <div className="flex flex-wrap items-center gap-2">
            {KEY_PRESETS.map((preset) => {
              const selected = !capturing && activePreset?.id === preset.id;
              return (
                <button
                  key={preset.id}
                  type="button"
                  aria-pressed={selected}
                  onClick={() => void applyPreset(preset)}
                  className={cn("ob-keycap ob-keycap-sm cursor-pointer", selected && "is-pressed")}
                >
                  {preset.isCaps && <span className="ob-keycap-led" />}
                  <span>{preset.cap}</span>
                </button>
              );
            })}
            <button
              type="button"
              aria-pressed={capturing || !activePreset}
              onClick={toggleCapture}
              className={cn(
                "h-[30px] cursor-pointer rounded-[8px] px-3 text-xs font-medium transition-colors",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                capturing
                  ? "border border-dashed border-[var(--color-accent)] bg-[var(--accent-soft)] text-[var(--color-accent-text)]"
                  : !activePreset
                    ? "border border-[var(--color-accent)] bg-[var(--accent-soft)] text-[var(--color-accent-text)]"
                    : "border border-[var(--color-line-strong)] bg-[var(--color-surface-hover)] text-[var(--color-fg)] hover:bg-[var(--color-surface-strong)]",
              )}
            >
              {capturing
                ? "请按下键盘按键（Esc 取消）…"
                : !activePreset && shortcutLabel(shortcut)
                  ? `${shortcutLabel(shortcut)}（重录）`
                  : "自定义录制"}
            </button>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--color-line)] pt-3">
          <span className="text-[13px] font-medium text-[var(--color-fg)]">触发方式</span>
          <div role="radiogroup" aria-label="触发方式" className="inline-flex rounded-[var(--radius-lg)] bg-[var(--color-surface-hover)] p-0.5 text-xs">
            <button
              type="button"
              role="radio"
              aria-checked={!pressHoldMode}
              onClick={() => void applyTriggerMode(false)}
              className={cn(
                "cursor-pointer rounded-[8px] px-3.5 py-1.5 transition-colors",
                !pressHoldMode
                  ? "bg-[var(--color-accent-control)] font-medium text-[var(--color-accent-control-contrast)]"
                  : "text-[var(--color-fg-muted)] hover:text-[var(--color-fg)]",
              )}
            >
              按一下（默认）
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={pressHoldMode}
              onClick={() => void applyTriggerMode(true)}
              className={cn(
                "cursor-pointer rounded-[8px] px-3.5 py-1.5 transition-colors",
                pressHoldMode
                  ? "bg-[var(--color-accent-control)] font-medium text-[var(--color-accent-control-contrast)]"
                  : "text-[var(--color-fg-muted)] hover:text-[var(--color-fg)]",
              )}
            >
              按住说话
            </button>
          </div>
        </div>
      </div>

      {(saveError || error) && (
        <p role="alert" className="text-center text-xs text-[var(--color-err)]">
          {saveError || error}
        </p>
      )}
    </div>
  );
}
