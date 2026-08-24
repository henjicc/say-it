import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Input, Select } from "@/components/ui/Input";
import { ColorInput } from "@/components/ui/ColorInput";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Slider } from "@/components/ui/Slider";
import { Switch } from "@/components/ui/Switch";
import { cn } from "@/lib/cn";
import type { FloatingOrbSettings } from "@/lib/tauri";
import { useThemeStore } from "@/store/useThemeStore";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import {
  FLOATING_ORB_GLASS_BORDER_RANGE,
  FLOATING_ORB_GLASS_TINT_RANGE,
} from "@/floating-orb/interaction";

function isHexColor(value: string) {
  return /^#?[0-9a-fA-F]{3}$/.test(value.trim()) || /^#?[0-9a-fA-F]{6}$/.test(value.trim());
}

function ThemeColorField({
  value,
  onChange,
  label,
  description,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  description: string;
}) {
  const [draft, setDraft] = useState(value);
  const invalid = draft.trim() !== "" && !isHexColor(draft);

  useEffect(() => {
    setDraft(value);
  }, [value]);

  const commit = () => {
    if (isHexColor(draft)) onChange(draft);
    else setDraft(value);
  };

  return (
    <div className="grid grid-cols-[2.75rem_1fr] gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-3">
      <ColorInput value={value} onChange={onChange} label={label} />
      <div className="min-w-0">
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-sm font-medium text-[var(--color-fg)]">{label}</p>
            <p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">{description}</p>
          </div>
          <span className="shrink-0 font-mono text-xs text-[var(--color-fg-subtle)]">{value}</span>
        </div>
        <Input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
          size="sm"
          className={cn("mt-2 font-mono", invalid && "border-[var(--color-err)]")}
          spellCheck={false}
        />
        {invalid && <p className="mt-1 text-xs text-[var(--color-err)]">请输入 3 或 6 位 Hex 颜色。</p>}
      </div>
    </div>
  );
}

export function SettingsAppearancePanel() {
  const theme = useThemeStore((s) => s.theme);
  const patch = useThemeStore((s) => s.patch);
  const reset = useThemeStore((s) => s.reset);
  const floatingOrb = useFloatingOrbStore((s) => s.settings);
  const floatingOrbError = useFloatingOrbStore((s) => s.error);
  const updateFloatingOrbAppearance = useFloatingOrbStore((s) => s.updateAppearance);

  return (
    <SettingsSection title="外观">
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-[1fr_18rem]">
        <div className="grid gap-3">
          <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-3">
            <p className="text-sm font-medium text-[var(--color-fg)]">整体色调</p>
            <p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">切换界面基础明暗。</p>
            <div className="mt-3 grid grid-cols-2 gap-1 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)] p-1">
              {[
                { value: "dark", label: "暗色调" },
                { value: "light", label: "亮色调" },
              ].map((option) => {
                const active = theme.tone === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => patch({ tone: option.value as "dark" | "light" })}
                    className={cn(
                      "h-[var(--control-h-sm)] rounded-[var(--radius-md)] text-sm transition-colors duration-[var(--dur-fast)]",
                      active
                        ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-contrast)]"
                        : "text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]",
                    )}
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>
          </div>

          <ThemeColorField
            value={theme.accent}
            onChange={(value) => patch({ accent: value })}
            label="强调色"
            description="按钮、选中项、焦点与滑块颜色"
          />

          <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-3">
            <p className="text-sm font-medium text-[var(--color-fg)]">背景色</p>
            <p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">可轻微跟随强调色，也可使用独立背景基色。</p>
            <div className="mt-3 grid grid-cols-2 gap-1 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)] p-1">
              {[
                { value: "followAccent", label: "跟随强调色" },
                { value: "custom", label: "自定义" },
              ].map((option) => {
                const active = theme.backgroundMode === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => patch({ backgroundMode: option.value as "followAccent" | "custom" })}
                    className={cn(
                      "h-[var(--control-h-sm)] rounded-[var(--radius-md)] text-sm transition-colors duration-[var(--dur-fast)]",
                      active
                        ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-contrast)]"
                        : "text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]",
                    )}
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>
          </div>

          {theme.backgroundMode === "custom" && (
            <ThemeColorField
              value={theme.background}
              onChange={(value) => patch({ background: value })}
              label="背景基色"
              description="用于主窗口、侧栏与悬浮窗口的基础背景"
            />
          )}
        </div>

        <div className="rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface-strong)] p-4">
          <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-bg)] p-3">
            <p className="text-xs text-[var(--color-fg-subtle)]">预览</p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button size="sm" variant="primary">
                主要操作
              </Button>
              <Button size="sm">次要操作</Button>
            </div>
            <div className="mt-4 h-2 rounded-full bg-[var(--color-surface-strong)]">
              <div className="h-full w-2/3 rounded-full bg-[var(--color-accent)]" />
            </div>
            <div className="mt-4 flex items-center gap-2 rounded-[var(--radius-lg)] bg-[var(--accent-soft-strong)] px-3 py-2 text-sm ring-1 ring-[var(--accent-ring)]">
              <span className="h-2 w-2 rounded-full bg-[var(--color-accent-light)]" />
              <span className="text-[var(--color-fg)]">当前选中状态</span>
            </div>
          </div>

          <Button className="mt-4 w-full" size="sm" onClick={reset}>
            恢复默认
          </Button>
        </div>
      </div>

      <div className="mt-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
        <div className="flex items-start justify-between gap-5">
          <div>
            <p className="text-sm font-medium text-[var(--color-fg)]">悬浮球系统毛玻璃</p>
            <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">使用 macOS Vibrancy 或 Windows Acrylic，并实时预览材质变化。</p>
          </div>
          <Switch
            checked={floatingOrb.glassEnabled}
            onChange={(glassEnabled) => void updateFloatingOrbAppearance({ glassEnabled }).catch(() => undefined)}
            aria-label="启用悬浮球系统毛玻璃"
          />
        </div>

        <div className={cn("mt-4 grid gap-4", !floatingOrb.glassEnabled && "opacity-45")}>
          <label className="grid gap-2 text-xs text-[var(--color-fg-muted)]">
            <span>macOS 系统材质</span>
            <Select
              size="sm"
              disabled={!floatingOrb.glassEnabled}
              value={floatingOrb.glassMaterial}
              onChange={(event) => void updateFloatingOrbAppearance({
                glassMaterial: event.target.value as FloatingOrbSettings["glassMaterial"],
              }).catch(() => undefined)}
            >
              <option value="underWindow">通透背景</option>
              <option value="content">柔和内容</option>
              <option value="sidebar">清晰侧栏</option>
            </Select>
          </label>
          <Slider
            label="底色强度"
            value={floatingOrb.glassTint}
            min={FLOATING_ORB_GLASS_TINT_RANGE.min}
            max={FLOATING_ORB_GLASS_TINT_RANGE.max}
            step={1}
            disabled={!floatingOrb.glassEnabled}
            format={(value) => `${value}%`}
            onChange={(glassTint) => void updateFloatingOrbAppearance({ glassTint }).catch(() => undefined)}
          />
          <Slider
            label="边框强度"
            value={floatingOrb.glassBorder}
            min={FLOATING_ORB_GLASS_BORDER_RANGE.min}
            max={FLOATING_ORB_GLASS_BORDER_RANGE.max}
            step={1}
            disabled={!floatingOrb.glassEnabled}
            format={(value) => `${value}%`}
            onChange={(glassBorder) => void updateFloatingOrbAppearance({ glassBorder }).catch(() => undefined)}
          />
        </div>
        <p className="mt-3 text-xs leading-relaxed text-[var(--color-fg-subtle)]">
          系统不开放连续的模糊半径；材质用于调整 macOS 模糊观感，Windows 主要受底色强度影响。
        </p>
        {floatingOrbError && <p className="mt-2 text-xs text-[var(--color-err)]" role="alert">保存悬浮球外观失败：{floatingOrbError}</p>}
      </div>
    </SettingsSection>
  );
}
