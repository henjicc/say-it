import { StrictMode, useEffect, useRef, useState, type CSSProperties } from "react";
import { createRoot } from "react-dom/client";
import { PanelTopOpen, Power, RotateCcw } from "lucide-react";
import { CMD, EVT, cmd, type AppSnapshot, type FloatingOrbSettings } from "@/lib/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { RangeInput } from "@/components/ui/RangeInput";
import { applyThemeToDocument, type AccentTheme } from "@/store/useThemeStore";
import {
  DEFAULT_FLOATING_ORB_APPEARANCE,
  FLOATING_ORB_OPACITY_RANGE,
  FLOATING_ORB_SIZE_RANGE,
  normalizeFloatingOrbAppearance,
} from "@/floating-orb/interaction";
import "@/index.css";
import "@/floating-orb-menu.css";

type Appearance = ReturnType<typeof normalizeFloatingOrbAppearance>;

function FloatingOrbMenuApp() {
  const [appearance, setAppearance] = useState<Appearance>(DEFAULT_FLOATING_ORB_APPEARANCE);
  const [error, setError] = useState("");
  const appearanceRef = useRef<Appearance>(DEFAULT_FLOATING_ORB_APPEARANCE);
  const updateRevision = useRef(0);

  const receive = (value: Partial<Appearance>) => {
    const next = normalizeFloatingOrbAppearance(value);
    appearanceRef.current = next;
    setAppearance(next);
  };

  const hideMenu = () => {
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
  const update = (patch: Partial<Appearance>) => {
    const revision = ++updateRevision.current;
    const next = normalizeFloatingOrbAppearance({ ...appearanceRef.current, ...patch });
    appearanceRef.current = next;
    setAppearance(next);
    setError("");
    void cmd<FloatingOrbSettings>(CMD.setFloatingOrbAppearance, { ...next })
      .then((value) => {
        if (revision === updateRevision.current) receive(value);
      })
      .catch((reason) => {
        if (revision === updateRevision.current) setError(String(reason));
      });
  };

  const disable = () => {
    setError("");
    void cmd(CMD.setFloatingOrbEnabled, { enabled: false }).catch((reason) => {
      setError(String(reason));
    });
  };

  const openMainWindow = () => {
    setError("");
    void cmd(CMD.floatingOrbOpenMainWindow).catch((reason) => setError(String(reason)));
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

      <div className="orb-menu-content">
        <label className="orb-menu-field">
          <span><span>大小</span><output>{appearance.size}px</output></span>
          <RangeInput
            ariaLabel="悬浮球大小"
            min={FLOATING_ORB_SIZE_RANGE.min}
            max={FLOATING_ORB_SIZE_RANGE.max}
            step={1}
            value={appearance.size}
            onChange={(size) => update({ size })}
          />
        </label>

        <label className="orb-menu-field">
          <span><span>不透明度</span><output>{appearance.opacity}%</output></span>
          <RangeInput
            ariaLabel="悬浮球不透明度"
            min={FLOATING_ORB_OPACITY_RANGE.min}
            max={FLOATING_ORB_OPACITY_RANGE.max}
            step={1}
            value={appearance.opacity}
            onChange={(opacity) => update({ opacity })}
          />
        </label>

        <div className="orb-menu-actions">
          <button type="button" className="orb-menu-action" onClick={openMainWindow}>
            <PanelTopOpen aria-hidden />
            <span><strong>打开软件主界面</strong><small>查看完整设置与历史记录</small></span>
          </button>
          <button
            type="button"
            className="orb-menu-action"
            onClick={() => update({
              size: DEFAULT_FLOATING_ORB_APPEARANCE.size,
              opacity: DEFAULT_FLOATING_ORB_APPEARANCE.opacity,
            })}
          >
            <RotateCcw aria-hidden />
            <span><strong>恢复显示默认值</strong><small>重置大小与不透明度</small></span>
          </button>
          <button type="button" className="orb-menu-action danger" onClick={disable}>
            <Power aria-hidden />
            <span><strong>关闭悬浮球</strong><small>可在主界面中重新开启</small></span>
          </button>
        </div>
        {error && <p className="orb-menu-error" role="alert">{error}</p>}
      </div>
    </main>
  );
}

createRoot(document.getElementById("floating-orb-menu-root")!).render(
  <StrictMode>
    <FloatingOrbMenuApp />
  </StrictMode>,
);
