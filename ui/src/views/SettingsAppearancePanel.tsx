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
import { backgroundSupportsText, useThemeStore } from "@/store/useThemeStore";
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
  validate,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  description: string;
  /** 颜色格式正确但不能用时返回原因，此时不保存。 */
  validate?: (value: string) => string | undefined;
}) {
  const [draft, setDraft] = useState(value);
  const invalid = draft.trim() !== "" && !isHexColor(draft);
  // 只校验新输入：已保存的值即使不满足也在生效，不在打开设置时就报错。
  const problem = isHexColor(draft) && draft !== value ? validate?.(draft) : undefined;

  useEffect(() => {
    setDraft(value);
  }, [value]);

  const commit = (next: string) => {
    if (!isHexColor(next)) setDraft(value);
    else if (!validate?.(next)) onChange(next);
  };

  return (
    <Row
      label={label}
      hint={description}
      message={(invalid || problem) && (
        <span role="alert" className="text-[var(--color-err)]">
          {invalid ? "请输入 3 位或 6 位颜色代码，例如 #369 或 #336699。" : problem}
        </span>
      )}
    >
      <Input
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => commit(draft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit(draft);
        }}
        size="sm"
        aria-label={`${label}代码`}
        className={cn("w-32 font-mono", (invalid || problem) && "border-[var(--color-err)]")}
        spellCheck={false}
      />
      <ColorInput
        value={value}
        onChange={(next) => {
          setDraft(next);
          commit(next);
        }}
        label={label}
        className="h-[var(--control-h-sm)] w-[var(--control-h-sm)]"
      />
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
            validate={(value) => backgroundSupportsText(value, theme.tone)
              ? undefined
              : theme.tone === "light"
                ? "亮色调需要足够浅的背景，这个颜色下文字会看不清。"
                : "暗色调需要足够深的背景，这个颜色下文字会看不清。"}
          />
        )}
        <div className="flex flex-wrap items-center gap-3 px-3.5 py-3" aria-label="外观预览">
          <Button size="sm" variant="primary">主要操作</Button>
          <Button size="sm">次要操作</Button>
          <div className="h-1.5 w-24 rounded-full bg-[var(--color-surface-strong)]">
            <div className="h-full w-2/3 rounded-full bg-[var(--color-accent)]" />
          </div>
          <div className="rounded-[var(--radius-md)] bg-[var(--color-accent-control)] text-[var(--color-accent-control-contrast)] px-2.5 py-1 text-xs">
            当前选中状态
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
