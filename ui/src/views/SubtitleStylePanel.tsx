import { useEffect, useState } from "react";
import { Group, Row, SliderRow, ROW_CONTROL_W } from "@/components/ui/Group";
import { Input, Select } from "@/components/ui/Input";
import { ColorInput } from "@/components/ui/ColorInput";
import { RangeInput } from "@/components/ui/RangeInput";
import { Switch } from "@/components/ui/Switch";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import { defaultSubtitleFontFamily, isMacOS } from "@/lib/platform";
import {
  useSubtitleStore,
  type SubtitleAnchor,
  type SubtitleAnimationEasing,
} from "@/store/useSubtitleStore";

const FALLBACK_FONTS = isMacOS
  ? [defaultSubtitleFontFamily, "Hiragino Sans GB", "Heiti SC"]
  : [defaultSubtitleFontFamily, "SimHei", "KaiTi", "Segoe UI"];


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

function ColorRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <Row label={label}>
      <ColorInput value={value} onChange={onChange} label={label} />
      <Input className="w-28" aria-label={`${label}色值`} value={value} onChange={(event) => onChange(event.target.value)} />
    </Row>
  );
}

function EasingRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: SubtitleAnimationEasing;
  onChange: (value: SubtitleAnimationEasing) => void;
}) {
  return (
    <Row label={label}>
      <Select className={ROW_CONTROL_W} aria-label={label} value={value} onChange={(event) => onChange(event.target.value as SubtitleAnimationEasing)}>
        {ANIMATION_EASING_OPTIONS.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </Select>
    </Row>
  );
}

export function SubtitleStylePanel() {
  const { prefs, patch } = useSubtitleStore();
  const systemFonts = useSystemFonts();
  const fontOptions = systemFonts.includes(prefs.fontFamily)
    ? systemFonts
    : [prefs.fontFamily, ...systemFonts];
  const outputToObs = prefs.obsOutputEnabled;
  const [obsPositionNoticeOpen, setObsPositionNoticeOpen] = useState(false);
  const obsPositionHint = "当前输出到 OBS，请在 OBS 画布中调整字幕的位置。";

  // 输出到 OBS 时位置控件被禁用；盖一层按钮，点击时说明去 OBS 里调整。
  const obsPositionGuard = outputToObs && (
    <button type="button" className="absolute inset-0 cursor-not-allowed" title={obsPositionHint} aria-label={obsPositionHint} onClick={() => setObsPositionNoticeOpen(true)} />
  );

  return (
    <>
      <Group title="文字">
        <Row label="字体">
          <Select
            className={ROW_CONTROL_W}
            aria-label="字体"
            searchable
            searchPlaceholder="搜索字体…"
            value={prefs.fontFamily}
            onChange={(event) => patch({ fontFamily: event.target.value })}
          >
            {fontOptions.map((font) => (
              <option key={font} value={font}>
                {font}
              </option>
            ))}
          </Select>
        </Row>
        <SliderRow label="字号" min={1.5} max={6} step={0.1} value={prefs.fontSizePercent} onChange={(fontSizePercent) => patch({ fontSizePercent })} format={(v) => `${v.toFixed(1)}%`} />
        <ColorRow label="字体颜色" value={prefs.textColor} onChange={(textColor) => patch({ textColor })} />
      </Group>

      <Group title="字幕框">
        <SliderRow label="字幕宽度" min={20} max={70} step={1} value={prefs.widthPercent} onChange={(widthPercent) => patch({ widthPercent })} format={(v) => `${v}%`} />
        {prefs.mode === "scroll" && (
          <SliderRow label="显示行数" min={1} max={4} step={1} value={prefs.lineCount} onChange={(lineCount) => patch({ lineCount })} format={(v) => `${v} 行`} />
        )}
        <ColorRow label="背景颜色" value={prefs.backgroundColor} onChange={(backgroundColor) => patch({ backgroundColor })} />
        <SliderRow label="背景不透明" min={0} max={100} step={1} value={prefs.backgroundOpacity} onChange={(backgroundOpacity) => patch({ backgroundOpacity })} format={(v) => `${v}%`} />
        <SliderRow label="圆角" min={0} max={36} step={1} value={prefs.rounded} onChange={(rounded) => patch({ rounded })} format={(v) => `${v}px`} />
      </Group>

      <Group title="位置">
        <Row label="位置" message={outputToObs && "字幕位置请在 OBS 中调整，其他样式会自动同步。"}>
          <div className="relative">
            <Select
              className={ROW_CONTROL_W}
              aria-label="位置"
              value={prefs.anchor}
              disabled={outputToObs}
              onChange={(event) => patch({ anchor: event.target.value as SubtitleAnchor })}
            >
              {Object.entries(anchorLabel).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </Select>
            {obsPositionGuard}
          </div>
        </Row>
        <div className="relative">
          <SliderRow disabled={outputToObs} label="位置偏移" min={-17} max={20} step={0.5} value={prefs.offsetYPercent} onChange={(offsetYPercent) => patch({ offsetYPercent })} format={(v) => `${v.toFixed(1)}%`} />
          {obsPositionGuard}
        </div>
      </Group>

      <Group title="动画" description="位移动画控制字幕切换和滚动时的移动效果；淡入动画让新出现的文字逐渐显示。">
        <Row label="位移动画">
          <Switch checked={prefs.motionEnabled} onChange={(motionEnabled) => patch({ motionEnabled })} label="位移动画" />
        </Row>
        <SliderRow label="位移时长" min={60} max={400} step={10} value={prefs.motionDurationMs} onChange={(motionDurationMs) => patch({ motionDurationMs })} format={(v) => `${v}ms`} />
        <EasingRow label="位移曲线" value={prefs.motionEasing} onChange={(motionEasing) => patch({ motionEasing })} />
        <Row label="淡入动画">
          <Switch checked={prefs.fadeEnabled} onChange={(fadeEnabled) => patch({ fadeEnabled })} label="淡入动画" />
        </Row>
        <SliderRow label="淡入时长" min={60} max={500} step={10} value={prefs.fadeDurationMs} onChange={(fadeDurationMs) => patch({ fadeDurationMs })} format={(v) => `${v}ms`} />
        <EasingRow label="淡入曲线" value={prefs.fadeEasing} onChange={(fadeEasing) => patch({ fadeEasing })} />
      </Group>

      <Modal
        open={obsPositionNoticeOpen}
        onClose={() => setObsPositionNoticeOpen(false)}
        showHeader={false}
        ariaLabel="请在 OBS 中调整字幕位置"
        className="max-w-md"
      >
        <div className="p-5">
          <h3 className="text-base font-semibold text-[var(--color-fg)]">请在 OBS 中调整字幕位置</h3>
          <p className="mt-4 text-sm leading-6 text-[var(--color-fg-muted)]">
            当前字幕输出目标为 OBS，因此“位置”和“位置偏移”由 OBS 画布控制。请在 OBS 中选中字幕源后，通过拖动、缩放或变换设置调整位置。
          </p>
          <div className="mt-5 flex justify-end">
            <Button variant="primary" onClick={() => setObsPositionNoticeOpen(false)}>关闭</Button>
          </div>
        </div>
      </Modal>
    </>
  );
}
