import { useEffect, useState } from "react";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Slider } from "@/components/ui/Slider";
import { Switch } from "@/components/ui/Switch";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { FormGrid } from "@/components/ui/FormGrid";
import { CMD, cmd } from "@/lib/tauri";
import {
  useSubtitleStore,
  type SubtitleAnchor,
  type SubtitleAnimationEasing,
} from "@/store/useSubtitleStore";

const FALLBACK_FONTS = ["Microsoft YaHei", "SimHei", "KaiTi", "Segoe UI"];

let cachedSystemFonts: string[] | null = null;

function useSystemFonts() {
  const [fonts, setFonts] = useState<string[]>(cachedSystemFonts ?? FALLBACK_FONTS);

  useEffect(() => {
    if (cachedSystemFonts) return;
    cmd<string[]>(CMD.listSystemFonts)
      .then((names) => {
        if (!names || names.length === 0) return;
        cachedSystemFonts = names;
        setFonts(names);
      })
      .catch(() => {
        /* 保留内置常用字体兜底 */
      });
  }, []);

  return fonts;
}

const anchorLabel: Record<SubtitleAnchor, string> = {
  bottom: "屏幕底部",
  center: "屏幕中部",
  top: "屏幕顶部",
};

const ANIMATION_EASING_OPTIONS: { value: SubtitleAnimationEasing; label: string }[] = [
  { value: "ease-out", label: "缓出（先快后慢）" },
  { value: "ease-in-out", label: "缓入缓出" },
  { value: "linear", label: "匀速" },
  { value: "ease-in", label: "缓入（先慢后快）" },
];

function ColorField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <Field label={label}>
      <div className="flex items-center gap-2">
        <input
          type="color"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          className="h-11 w-12 flex-none cursor-pointer rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] p-1"
          aria-label={label}
        />
        <Input value={value} onChange={(event) => onChange(event.target.value)} />
      </div>
    </Field>
  );
}

export function SubtitleStylePanel() {
  const { prefs, patch, obsOutputActive } = useSubtitleStore();
  const systemFonts = useSystemFonts();

  return (
    <div className="flex flex-col gap-7">
      <SettingsSection title="字幕样式">
        {obsOutputActive && (
          <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
            正在输出到 OBS：字幕位置直接在 OBS 画布中拖动调整，"位置"与"位置偏移"已隐藏；其余样式实时同步到 OBS。
          </p>
        )}
        <FormGrid>
          <Field layout="row" label="字体">
            <Select
              searchable
              searchPlaceholder="搜索字体…"
              value={prefs.fontFamily}
              onChange={(event) => patch({ fontFamily: event.target.value })}
            >
              {systemFonts.map((font) => (
                <option key={font} value={font}>
                  {font}
                </option>
              ))}
            </Select>
          </Field>
          {!obsOutputActive && (
            <Field layout="row" label="位置">
              <Select
                value={prefs.anchor}
                onChange={(event) => patch({ anchor: event.target.value as SubtitleAnchor })}
              >
                {Object.entries(anchorLabel).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </Select>
            </Field>
          )}
        </FormGrid>

        <FormGrid>
          <Slider label="字号" min={1.5} max={6} step={0.1} value={prefs.fontSizePercent} onChange={(fontSizePercent) => patch({ fontSizePercent })} format={(v) => `${v.toFixed(1)}%`} />
          {prefs.mode === "scroll" && (
            <Slider label="显示行数" min={1} max={4} step={1} value={prefs.lineCount} onChange={(lineCount) => patch({ lineCount })} format={(v) => `${v} 行`} />
          )}
          <Slider label="字幕宽度" min={20} max={70} step={1} value={prefs.widthPercent} onChange={(widthPercent) => patch({ widthPercent })} format={(v) => `${v}%`} />
          {!obsOutputActive && (
            <Slider label="位置偏移" min={-17} max={20} step={0.5} value={prefs.offsetYPercent} onChange={(offsetYPercent) => patch({ offsetYPercent })} format={(v) => `${v.toFixed(1)}%`} />
          )}
          <Slider label="背景不透明" min={0} max={100} step={1} value={prefs.backgroundOpacity} onChange={(backgroundOpacity) => patch({ backgroundOpacity })} format={(v) => `${v}%`} />
          <Slider label="圆角" min={0} max={36} step={1} value={prefs.rounded} onChange={(rounded) => patch({ rounded })} format={(v) => `${v}px`} />
        </FormGrid>

        <FormGrid>
          <ColorField label="字体颜色" value={prefs.textColor} onChange={(textColor) => patch({ textColor })} />
          <ColorField label="背景颜色" value={prefs.backgroundColor} onChange={(backgroundColor) => patch({ backgroundColor })} />
        </FormGrid>
      </SettingsSection>

      <SettingsSection title="字幕动画">
        <p className="text-xs text-[var(--color-fg-subtle)]">
          位移动画用于单句替换的左右平移、滚动累积的上下滚动；淡入动画用于新增文字出现时的不透明度过渡。
        </p>
        <FormGrid>
          <Field layout="row" label="位移动画">
            <Switch
              checked={prefs.motionEnabled}
              onChange={(motionEnabled) => patch({ motionEnabled })}
              label="位移动画"
            />
          </Field>
          <Field layout="row" label="淡入动画">
            <Switch checked={prefs.fadeEnabled} onChange={(fadeEnabled) => patch({ fadeEnabled })} label="淡入动画" />
          </Field>
        </FormGrid>

        <FormGrid>
          <Slider
            label="位移时长"
            min={60}
            max={400}
            step={10}
            value={prefs.motionDurationMs}
            onChange={(motionDurationMs) => patch({ motionDurationMs })}
            format={(v) => `${v}ms`}
          />
          <Slider
            label="淡入时长"
            min={60}
            max={500}
            step={10}
            value={prefs.fadeDurationMs}
            onChange={(fadeDurationMs) => patch({ fadeDurationMs })}
            format={(v) => `${v}ms`}
          />
        </FormGrid>

        <FormGrid>
          <Field layout="row" label="位移曲线">
            <Select
              value={prefs.motionEasing}
              onChange={(event) => patch({ motionEasing: event.target.value as SubtitleAnimationEasing })}
            >
              {ANIMATION_EASING_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field layout="row" label="淡入曲线">
            <Select
              value={prefs.fadeEasing}
              onChange={(event) => patch({ fadeEasing: event.target.value as SubtitleAnimationEasing })}
            >
              {ANIMATION_EASING_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
        </FormGrid>
      </SettingsSection>
    </div>
  );
}
