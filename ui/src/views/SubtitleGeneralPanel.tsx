import { useEffect, useState } from "react";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { ClearIcon } from "@/components/ui/icons";
import { Slider } from "@/components/ui/Slider";
import { Switch } from "@/components/ui/Switch";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { FormGrid } from "@/components/ui/FormGrid";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import {
  syncSubtitleIndicator,
  showSubtitlePreview,
  hideSubtitlePreview,
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
} from "@/features/subtitles/controller";
import {
  useSubtitleStore,
  buildSubtitleSource,
  parseSubtitleSource,
  type SubtitleAnchor,
  type SubtitleMode,
  type SubtitleAnimationEasing,
} from "@/store/useSubtitleStore";
import { useAudioDevices } from "@/features/audio/devices";
import { useDictPrefs } from "@/store/useDictPrefs";
import { SUBTITLE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";

const FALLBACK_FONTS = ["Microsoft YaHei", "SimHei", "KaiTi", "Segoe UI"];
const shortcutActionButtonClassName = "min-h-[var(--control-h)] shrink-0 self-stretch";

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

function MonitorIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className="h-4 w-4"
      aria-hidden
    >
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M8 20h8M12 16v4" />
    </svg>
  );
}

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

export function SubtitleGeneralPanel() {
  const { prefs, running, capturing, shortcutLabel, patch } = useSubtitleStore();
  const [previewOpen, setPreviewOpen] = useState(false);
  const systemFonts = useSystemFonts();
  const { inputs, outputs } = useAudioDevices();
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const dictPatch = useDictPrefs((s) => s.patch);

  // 麦克风设备是语音输入和实时字幕共用的全局偏好（同一个后端采集单例），
  // 这里下拉框选中的"麦克风"具体设备始终跟着这个全局值走，而不是自己单独存一份。
  const parsedSource = parseSubtitleSource(prefs.source);
  const sourceSelectValue =
    parsedSource.kind === "mic" ? buildSubtitleSource("mic", micDeviceId || undefined) : prefs.source;
  const onSourceChange = (nextValue: string) => {
    const next = parseSubtitleSource(nextValue);
    if (next.kind === "mic") dictPatch({ micDeviceId: next.deviceName || "" });
    patch({ source: nextValue });
  };

  // 真正运行、或预览开着时，持续把样式变化同步到悬浮窗。
  useEffect(() => {
    if (running || previewOpen) syncSubtitleIndicator(prefs);
  }, [prefs, running, previewOpen]);

  // 预览开关的显示/隐藏生命周期：打开时在桌面实际位置展示示例文本；
  // 关闭、真正开始字幕、或离开本页面时都要收起悬浮窗（不影响正在运行的真实字幕）。
  useEffect(() => {
    if (running || !previewOpen) return undefined;
    // 只在开关/运行状态变化时触发一次；样式跟随交给上面那个 effect。
    showSubtitlePreview(prefs);
    return () => {
      hideSubtitlePreview();
    };
  }, [previewOpen, running]);

  return (
    <div className="flex flex-col gap-7">
      <SettingsSection title="基础设置">
        <FormGrid>
          <Field layout="row" label="识别模型">
            <Select
              value={prefs.asrModel}
              disabled={running}
              onChange={(event) => patch({ asrModel: event.target.value })}
            >
              {SUBTITLE_ASR_MODEL_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field layout="row" label="全局快捷键">
            <div className="flex items-stretch gap-2">
              <div className="relative min-w-0 flex-1">
                <Input
                  readOnly
                  value={capturing ? "请按下按键…" : shortcutLabel}
                  placeholder="未设置"
                  className={cn(
                    capturing && "border-[var(--accent-ring)]",
                    !capturing && shortcutLabel && "pr-9",
                  )}
                />
                {!capturing && shortcutLabel && (
                  <button
                    type="button"
                    aria-label="清除快捷键"
                    onClick={clearSubtitleShortcut}
                    className="absolute right-2 top-1/2 grid h-7 w-7 -translate-y-1/2 place-items-center rounded-[var(--radius-md)] text-[var(--color-fg-subtle)] transition-colors hover:bg-[var(--color-surface-strong)] hover:text-[var(--color-fg)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]"
                  >
                    <ClearIcon />
                  </button>
                )}
              </div>
              <Button className={shortcutActionButtonClassName} onClick={startSubtitleShortcutCapture}>
                {capturing ? "取消" : "修改"}
              </Button>
            </div>
          </Field>
          <Field layout="row" label="声音来源">
            <Select
              searchable={inputs.length + outputs.length > 5}
              searchPlaceholder="搜索设备…"
              value={sourceSelectValue}
              disabled={running}
              onChange={(event) => onSourceChange(event.target.value)}
            >
              <option value={buildSubtitleSource("mic")}>麦克风（默认）</option>
              <option value={buildSubtitleSource("system")}>系统音频（默认）</option>
              {inputs.length > 0 && (
                <option value="__group_inputs" disabled>
                  — 输入设备 —
                </option>
              )}
              {inputs.map((device) => (
                <option key={`in:${device.name}`} value={buildSubtitleSource("mic", device.name)}>
                  {device.name}
                </option>
              ))}
              {outputs.length > 0 && (
                <option value="__group_outputs" disabled>
                  — 输出设备 —
                </option>
              )}
              {outputs.map((device) => (
                <option key={`out:${device.name}`} value={buildSubtitleSource("system", device.name)}>
                  {device.name}
                </option>
              ))}
            </Select>
          </Field>
          <Field layout="row" label="字幕更新方式">
            <Select
              value={prefs.mode}
              onChange={(event) => patch({ mode: event.target.value as SubtitleMode })}
            >
              <option value="scroll">滚动累积</option>
              <option value="replace">单句替换</option>
            </Select>
          </Field>
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
        </FormGrid>
      </SettingsSection>

      <SettingsSection title="字幕样式">
        <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
          <span className="flex-none text-[var(--color-fg-subtle)]">
            <MonitorIcon />
          </span>
          <span className="flex-none text-sm font-medium text-[var(--color-fg)]">调整预览</span>
          <span
            className="min-w-0 flex-1 truncate text-xs text-[var(--color-fg-subtle)]"
            title={
              running
                ? "字幕运行中，桌面悬浮窗已实时显示真实内容。"
                : "打开后会在桌面实际位置显示示例字幕，方便调整样式，不会启动麦克风识别。"
            }
          >
            {running
              ? "字幕运行中，桌面悬浮窗已实时显示真实内容。"
              : "打开后会在桌面实际位置显示示例字幕，方便调整样式，不会启动麦克风识别。"}
          </span>
          <Switch checked={previewOpen} onChange={setPreviewOpen} disabled={running} label="调整预览" />
        </div>

        <div className="grid gap-x-8 gap-y-4 sm:grid-cols-2">
          <Slider label="字号" min={1.5} max={6} step={0.1} value={prefs.fontSizePercent} onChange={(fontSizePercent) => patch({ fontSizePercent })} format={(v) => `${v.toFixed(1)}%`} />
          {prefs.mode === "scroll" && (
            <Slider label="显示行数" min={1} max={4} step={1} value={prefs.lineCount} onChange={(lineCount) => patch({ lineCount })} format={(v) => `${v} 行`} />
          )}
          <Slider label="字幕宽度" min={20} max={70} step={1} value={prefs.widthPercent} onChange={(widthPercent) => patch({ widthPercent })} format={(v) => `${v}%`} />
          <Slider label="位置偏移" min={-17} max={20} step={0.5} value={prefs.offsetYPercent} onChange={(offsetYPercent) => patch({ offsetYPercent })} format={(v) => `${v.toFixed(1)}%`} />
          <Slider label="背景不透明" min={0} max={100} step={1} value={prefs.backgroundOpacity} onChange={(backgroundOpacity) => patch({ backgroundOpacity })} format={(v) => `${v}%`} />
          <Slider label="圆角" min={0} max={36} step={1} value={prefs.rounded} onChange={(rounded) => patch({ rounded })} format={(v) => `${v}px`} />
        </div>

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

        <div className="grid gap-x-8 gap-y-4 sm:grid-cols-2">
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
        </div>

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
