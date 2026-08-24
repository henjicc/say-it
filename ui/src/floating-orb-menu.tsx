import { StrictMode, useEffect, useRef, useState, type CSSProperties } from "react";
import { createRoot } from "react-dom/client";
import { CMD, cmd, type FloatingOrbSettings } from "@/lib/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import {
  DEFAULT_FLOATING_ORB_APPEARANCE,
  FLOATING_ORB_GLASS_BORDER_RANGE,
  FLOATING_ORB_GLASS_TINT_RANGE,
  FLOATING_ORB_OPACITY_RANGE,
  FLOATING_ORB_SIZE_RANGE,
  normalizeFloatingOrbAppearance,
} from "@/floating-orb/interaction";
import "@/floating-orb-menu.css";

type Appearance = ReturnType<typeof normalizeFloatingOrbAppearance>;

function FloatingOrbMenuApp() {
  const [appearance, setAppearance] = useState<Appearance>(DEFAULT_FLOATING_ORB_APPEARANCE);
  const [error, setError] = useState("");
  const appearanceRef = useRef<Appearance>(DEFAULT_FLOATING_ORB_APPEARANCE);

  const receive = (value: Partial<Appearance>) => {
    const next = normalizeFloatingOrbAppearance(value);
    appearanceRef.current = next;
    setAppearance(next);
  };

  useEffect(() => {
    void cmd<FloatingOrbSettings>(CMD.getFloatingOrbSettings).then(receive).catch((reason) => {
      setError(String(reason));
    });
    const hide = () => void cmd(CMD.hideFloatingOrbMenu).catch(() => undefined);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") hide();
    };
    window.addEventListener("blur", hide);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("blur", hide);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  useTauriEvent<Partial<Appearance>>("floating-orb-config", receive);

  const update = (patch: Partial<Appearance>) => {
    const next = normalizeFloatingOrbAppearance({ ...appearanceRef.current, ...patch });
    appearanceRef.current = next;
    setAppearance(next);
    setError("");
    void cmd<FloatingOrbSettings>(CMD.setFloatingOrbAppearance, { ...next })
      .then(receive)
      .catch((reason) => setError(String(reason)));
  };

  const disable = () => {
    setError("");
    void cmd(CMD.setFloatingOrbEnabled, { enabled: false }).catch((reason) => {
      setError(String(reason));
    });
  };

  return (
    <main
      className={`orb-menu-panel${appearance.glassEnabled ? " glass" : ""}`}
      style={{
        "--glass-tint": appearance.glassTint / 100,
        "--glass-border": appearance.glassBorder / 100,
      } as CSSProperties}
    >
      <header className="orb-menu-header">
        <strong>悬浮球设置</strong>
        <button
          type="button"
          className="orb-menu-close"
          aria-label="关闭设置面板"
          onClick={() => void cmd(CMD.hideFloatingOrbMenu)}
        >
          ×
        </button>
      </header>

      <label className="orb-menu-field">
        <span><span>大小</span><output>{appearance.size}px</output></span>
        <input
          type="range"
          min={FLOATING_ORB_SIZE_RANGE.min}
          max={FLOATING_ORB_SIZE_RANGE.max}
          step={1}
          value={appearance.size}
          onChange={(event) => update({ size: Number(event.target.value) })}
        />
      </label>

      <label className="orb-menu-field">
        <span><span>不透明度</span><output>{appearance.opacity}%</output></span>
        <input
          type="range"
          min={FLOATING_ORB_OPACITY_RANGE.min}
          max={FLOATING_ORB_OPACITY_RANGE.max}
          step={1}
          value={appearance.opacity}
          onChange={(event) => update({ opacity: Number(event.target.value) })}
        />
      </label>

      <div className="orb-menu-toggle-row">
        <span>
          <strong>系统毛玻璃</strong>
          <small>macOS Vibrancy / Windows Acrylic</small>
        </span>
        <label className="orb-menu-switch">
          <input
            type="checkbox"
            checked={appearance.glassEnabled}
            onChange={(event) => update({ glassEnabled: event.target.checked })}
          />
          <span aria-hidden="true" />
          <span className="orb-menu-sr-only">启用系统毛玻璃</span>
        </label>
      </div>

      <section
        className={`orb-menu-glass-tuning${appearance.glassEnabled ? "" : " disabled"}`}
        aria-label="毛玻璃调校"
        aria-disabled={!appearance.glassEnabled}
      >
        <label className="orb-menu-field">
          <span><span>macOS 系统材质</span></span>
          <select
            disabled={!appearance.glassEnabled}
            value={appearance.glassMaterial}
            onChange={(event) => update({
              glassMaterial: event.target.value as Appearance["glassMaterial"],
            })}
          >
            <option value="underWindow">通透背景</option>
            <option value="content">柔和内容</option>
            <option value="sidebar">清晰侧栏</option>
          </select>
        </label>

        <label className="orb-menu-field">
          <span><span>底色强度</span><output>{appearance.glassTint}%</output></span>
          <input
            type="range"
            disabled={!appearance.glassEnabled}
            min={FLOATING_ORB_GLASS_TINT_RANGE.min}
            max={FLOATING_ORB_GLASS_TINT_RANGE.max}
            step={1}
            value={appearance.glassTint}
            onChange={(event) => update({ glassTint: Number(event.target.value) })}
          />
        </label>

        <label className="orb-menu-field">
          <span><span>边框强度</span><output>{appearance.glassBorder}%</output></span>
          <input
            type="range"
            disabled={!appearance.glassEnabled}
            min={FLOATING_ORB_GLASS_BORDER_RANGE.min}
            max={FLOATING_ORB_GLASS_BORDER_RANGE.max}
            step={1}
            value={appearance.glassBorder}
            onChange={(event) => update({ glassBorder: Number(event.target.value) })}
          />
        </label>

        <p className="orb-menu-glass-hint">
          {appearance.glassEnabled
            ? "系统不开放连续的模糊半径，材质用于调整模糊观感；Windows 主要受底色强度影响。"
            : "开启系统毛玻璃后可实时调校。"}
        </p>
      </section>

      {error && <p className="orb-menu-error" role="alert">{error}</p>}
      <button type="button" className="orb-menu-disable" onClick={disable}>关闭悬浮球</button>
    </main>
  );
}

createRoot(document.getElementById("floating-orb-menu-root")!).render(
  <StrictMode>
    <FloatingOrbMenuApp />
  </StrictMode>,
);
