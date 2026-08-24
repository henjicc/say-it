import { StrictMode, useEffect, useRef, useState, type CSSProperties } from "react";
import { createRoot } from "react-dom/client";
import { ChevronDown } from "lucide-react";
import { CMD, EVT, cmd, type AppSnapshot, type FloatingOrbSettings } from "@/lib/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { Select } from "@/components/ui/Input";
import { applyThemeToDocument, type AccentTheme } from "@/store/useThemeStore";
import {
  DEFAULT_FLOATING_ORB_APPEARANCE,
  FLOATING_ORB_GLASS_BORDER_RANGE,
  FLOATING_ORB_GLASS_TINT_RANGE,
  FLOATING_ORB_OPACITY_RANGE,
  FLOATING_ORB_SIZE_RANGE,
  normalizeFloatingOrbAppearance,
} from "@/floating-orb/interaction";
import "@/index.css";
import "@/floating-orb-menu.css";

type Appearance = ReturnType<typeof normalizeFloatingOrbAppearance>;

function FloatingOrbMenuApp() {
  const [appearance, setAppearance] = useState<Appearance>(DEFAULT_FLOATING_ORB_APPEARANCE);
  const [tuningOpen, setTuningOpen] = useState(false);
  const [error, setError] = useState("");
  const appearanceRef = useRef<Appearance>(DEFAULT_FLOATING_ORB_APPEARANCE);

  const receive = (value: Partial<Appearance>) => {
    const next = normalizeFloatingOrbAppearance(value);
    appearanceRef.current = next;
    setAppearance(next);
  };

  const hideMenu = () => {
    setTuningOpen(false);
    void cmd(CMD.hideFloatingOrbMenu).catch(() => undefined);
  };

  useEffect(() => {
    void cmd<AppSnapshot>(CMD.getAppSnapshot)
      .then((snapshot) => applyThemeToDocument(snapshot.settings.theme as Partial<AccentTheme>))
      .catch(() => undefined);
    void cmd<FloatingOrbSettings>(CMD.getFloatingOrbSettings).then(receive).catch((reason) => {
      setError(String(reason));
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") hideMenu();
    };
    window.addEventListener("blur", hideMenu);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("blur", hideMenu);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  useTauriEvent<Partial<Appearance>>("floating-orb-config", receive);
  useTauriEvent<Partial<AccentTheme>>(EVT.themeChanged, applyThemeToDocument);
  useTauriEvent<{ expanded?: boolean }>("floating-orb-menu-expanded", (payload) => {
    setTuningOpen(payload.expanded === true);
  });

  const setTuningExpanded = (expanded: boolean) => {
    setTuningOpen(expanded);
    setError("");
    void cmd(CMD.setFloatingOrbMenuExpanded, { expanded }).catch((reason) => {
      setTuningOpen(!expanded);
      setError(String(reason));
    });
  };

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
        "--glass-tint": `${appearance.glassTint}%`,
        "--glass-border": `${appearance.glassBorder}%`,
      } as CSSProperties}
    >
      <header className="orb-menu-header">
        <strong>悬浮球设置</strong>
        <button
          type="button"
          className="orb-menu-close"
          aria-label="关闭设置面板"
          onClick={hideMenu}
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
            onChange={(event) => {
              if (!event.target.checked && tuningOpen) setTuningExpanded(false);
              update({ glassEnabled: event.target.checked });
            }}
          />
          <span aria-hidden="true" />
          <span className="orb-menu-sr-only">启用系统毛玻璃</span>
        </label>
      </div>

      <button
        type="button"
        className="orb-menu-disclosure"
        disabled={!appearance.glassEnabled}
        aria-expanded={tuningOpen}
        onClick={() => setTuningExpanded(!tuningOpen)}
      >
        <span>材质调节</span>
        <ChevronDown className={tuningOpen ? "expanded" : ""} aria-hidden />
      </button>

      {tuningOpen && (
        <section className="orb-menu-glass-tuning" aria-label="毛玻璃调校">
          <div className="orb-menu-field">
            <span><span>macOS 系统材质</span></span>
            <Select
              size="sm"
              aria-label="macOS 系统材质"
              value={appearance.glassMaterial}
              onChange={(event) => update({
                glassMaterial: event.target.value as Appearance["glassMaterial"],
              })}
            >
              <option value="underWindow">通透背景</option>
              <option value="content">柔和内容</option>
              <option value="sidebar">清晰侧栏</option>
            </Select>
          </div>

          <label className="orb-menu-field">
            <span><span>底色强度</span><output>{appearance.glassTint}%</output></span>
            <input
              type="range"
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
              min={FLOATING_ORB_GLASS_BORDER_RANGE.min}
              max={FLOATING_ORB_GLASS_BORDER_RANGE.max}
              step={1}
              value={appearance.glassBorder}
              onChange={(event) => update({ glassBorder: Number(event.target.value) })}
            />
          </label>

          <p className="orb-menu-glass-hint">
            系统不开放连续的模糊半径，材质用于调整模糊观感；Windows 主要受底色强度影响。
          </p>
        </section>
      )}

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
