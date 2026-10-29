import { useEffect, useState } from "react";
import { InspectorBody } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { ColorInput } from "@/components/ui/ColorInput";
import { Group, Row, SliderRow } from "@/components/ui/Group";
import { RangeInput } from "@/components/ui/RangeInput";
import { Switch } from "@/components/ui/Switch";
import { Tabs } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { useThemeStore } from "@/store/useThemeStore";
import { useFloatingOrbStore } from "@/store/useFloatingOrbStore";
import {
  FLOATING_ORB_GLASS_BORDER_RANGE,
  FLOATING_ORB_GLASS_TINT_RANGE,
} from "@/floating-orb/interaction";

function isHexColor(value: string) {
  return /^#?[0-9a-fA-F]{3}$/.test(value.trim()) || /^#?[0-9a-fA-F]{6}$/.test(value.trim());
}

function ThemeColorRow({
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
    <Row
      label={label}
      hint={description}
      message={invalid && <span role="alert" className="text-[var(--color-err)]">请输入 3 位或 6 位颜色代码，例如 #369 或 #336699。</span>}
    >
      <Input
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit();
        }}
        size="sm"
        aria-label={`${label}代码`}
        className={cn("w-32 font-mono", invalid && "border-[var(--color-err)]")}
        spellCheck={false}
      />
      <ColorInput value={value} onChange={onChange} label={label} className="h-[var(--control-h-sm)] w-[var(--control-h-sm)]" />
    </Row>
  );
}

export function SettingsAppearancePanel() {
  const theme = useThemeStore((s) => s.theme);
  const patch = useThemeStore((s) => s.patch);
  const reset = useThemeStore((s) => s.reset);
  const floatingOrb = useFloatingOrbStore((s) => s.settings);
  const floatingOrbError = useFloatingOrbStore((s) => s.error);
  const updateFloatingOrbAppearance = useFloatingOrbStore((s) => s.updateAppearance);
  const glassOff = !floatingOrb.glassEnabled;

  return (
    <InspectorBody>
      <Group title="外观" right={<Button size="sm" onClick={reset}>恢复默认</Button>}>
        <Row label="整体色调" hint="选择深色或浅色界面。">
          <Tabs
            variant="view"
            ariaLabel="整体色调"
            tabs={[{ key: "dark", label: "暗色调" }, { key: "light", label: "亮色调" }]}
            active={theme.tone}
            onChange={(tone) => patch({ tone: tone as "dark" | "light" })}
          />
        </Row>
        <ThemeColorRow
          value={theme.accent}
          onChange={(value) => patch({ accent: value })}
          label="强调色"
          description="按钮、选中项、焦点与滑块颜色"
        />
        <Row label="背景色" hint="使用柔和背景，或自选背景颜色。">
          <Tabs
            variant="view"
            ariaLabel="背景色"
            tabs={[{ key: "followAccent", label: "柔和背景" }, { key: "custom", label: "自定义" }]}
            active={theme.backgroundMode}
            onChange={(mode) => patch({ backgroundMode: mode as "followAccent" | "custom" })}
          />
        </Row>
        {theme.backgroundMode === "custom" && (
          <ThemeColorRow
            value={theme.background}
            onChange={(value) => patch({ background: value })}
            label="背景基色"
            description="用于主窗口、侧栏与悬浮窗口的基础背景"
          />
        )}
        <div className="flex flex-wrap items-center gap-3 px-3.5 py-3" aria-label="外观预览">
          <Button size="sm" variant="primary">主要操作</Button>
          <Button size="sm">次要操作</Button>
          <div className="h-1.5 w-24 rounded-full bg-[var(--color-surface-strong)]">
            <div className="h-full w-2/3 rounded-full bg-[var(--color-accent)]" />
          </div>
          <div className="flex items-center gap-2 rounded-[var(--radius-md)] bg-[var(--accent-soft-strong)] px-2.5 py-1 text-xs ring-1 ring-[var(--accent-ring)]">
            <span className="h-1.5 w-1.5 rounded-full bg-[var(--color-accent-light)]" />
            <span className="text-[var(--color-fg)]">当前选中状态</span>
          </div>
        </div>
      </Group>

      <Group title="系统毛玻璃">
        <Row
          label="全局系统毛玻璃"
          message={floatingOrbError && <span role="alert" className="text-[var(--color-err)]">保存悬浮球外观失败：{floatingOrbError}</span>}
        >
          <Switch
            checked={floatingOrb.glassEnabled}
            onChange={(glassEnabled) => void updateFloatingOrbAppearance({ glassEnabled }).catch(() => undefined)}
            label="启用全局系统毛玻璃"
          />
        </Row>
        <Row label="材质" disabled={glassOff}>
          <span className="text-[13px] text-[var(--color-fg-muted)]">简单材质系统</span>
        </Row>
        <SliderRow
          label="底色强度"
          value={floatingOrb.glassTint}
          min={FLOATING_ORB_GLASS_TINT_RANGE.min}
          max={FLOATING_ORB_GLASS_TINT_RANGE.max}
          step={1}
          format={(v) => `${v}%`}
          disabled={glassOff}
          onChange={(glassTint) => void updateFloatingOrbAppearance({ glassTint }).catch(() => undefined)}
        />
        <SliderRow
          label="边框强度"
          value={floatingOrb.glassBorder}
          min={FLOATING_ORB_GLASS_BORDER_RANGE.min}
          max={FLOATING_ORB_GLASS_BORDER_RANGE.max}
          step={1}
          format={(v) => `${v}%`}
          disabled={glassOff}
          onChange={(glassBorder) => void updateFloatingOrbAppearance({ glassBorder }).catch(() => undefined)}
        />
      </Group>
    </InspectorBody>
  );
}
