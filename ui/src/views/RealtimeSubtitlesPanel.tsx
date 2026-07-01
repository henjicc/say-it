import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
import { CheckField, Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Slider } from "@/components/ui/Slider";
import { cn } from "@/lib/cn";
import { CMD, cmd } from "@/lib/tauri";
import {
  syncSubtitleIndicator,
  showSubtitlePreview,
  hideSubtitlePreview,
  toggleSubtitles,
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
} from "@/features/subtitles/controller";
import {
  useSubtitleStore,
  buildSubtitleSource,
  parseSubtitleSource,
  type SubtitleAnchor,
  type SubtitleMode,
} from "@/store/useSubtitleStore";
import { useAudioDevices } from "@/features/audio/devices";
import { useDictPrefs } from "@/store/useDictPrefs";

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

const toneClass: Record<string, string> = {
  "": "text-white/50",
  ok: "text-[#25c36f]",
  err: "text-[#ff6b6b]",
};

const anchorLabel: Record<SubtitleAnchor, string> = {
  bottom: "屏幕底部",
  center: "屏幕中部",
  top: "屏幕顶部",
};

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
          className="h-10 w-12 cursor-pointer rounded-xl border border-white/10 bg-white/5 p-1"
          aria-label={label}
        />
        <Input value={value} onChange={(event) => onChange(event.target.value)} />
      </div>
    </Field>
  );
}

export function RealtimeSubtitlesPanel() {
  const { prefs, running, statusText, statusTone, capturing, shortcutLabel, patch } = useSubtitleStore();
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
    <Card>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <CardTitle>实时字幕</CardTitle>
          <CardDescription>
            持续识别语音并在屏幕上显示字幕，适合会议、网课、视频和临时转写。
          </CardDescription>
        </div>
        <Button variant={running ? "danger" : "primary"} onClick={toggleSubtitles}>
          {running ? "停止字幕" : "开始字幕"}
        </Button>
      </div>

      <p className={cn("mt-3 text-sm", toneClass[statusTone])}>{statusText}</p>

      <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label="全局快捷键">
          <div className="flex gap-2">
            <Input
              readOnly
              value={capturing ? "请按下按键…" : shortcutLabel}
              placeholder="未设置"
              className={cn(capturing && "border-white/40")}
            />
            <Button className="shrink-0" onClick={startSubtitleShortcutCapture}>
              {capturing ? "取消" : "修改"}
            </Button>
            {!capturing && shortcutLabel && (
              <Button className="shrink-0" onClick={clearSubtitleShortcut}>
                清除
              </Button>
            )}
          </div>
        </Field>
      </div>

      <div className="mt-5 grid gap-4 md:grid-cols-2">
        <Field label="声音来源">
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
        <Field label="字幕更新方式">
          <Select
            value={prefs.mode}
            onChange={(event) => patch({ mode: event.target.value as SubtitleMode })}
          >
            <option value="scroll">滚动累积</option>
            <option value="replace">单句替换</option>
          </Select>
        </Field>
      </div>

      <div className="mt-4 grid gap-4 md:grid-cols-2">
        <Field label="字体">
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
        <Field label="位置">
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
      </div>

      <div className="mt-5">
        <CheckField checked={previewOpen} onChange={setPreviewOpen} disabled={running}>
          调整预览
        </CheckField>
        <p className="mt-1.5 text-xs text-white/40">
          {running
            ? "字幕运行中，桌面悬浮窗已实时显示真实内容。"
            : "打开后会在桌面实际位置显示示例字幕，方便调整样式，不会启动麦克风识别。"}
        </p>
      </div>

      <div className="mt-5 grid gap-x-8 gap-y-4 md:grid-cols-2">
        <Slider label="字号" min={1.5} max={6} step={0.1} value={prefs.fontSizePercent} onChange={(fontSizePercent) => patch({ fontSizePercent })} format={(v) => `${v.toFixed(1)}%`} />
        {prefs.mode === "scroll" && (
          <Slider label="显示行数" min={1} max={4} step={1} value={prefs.lineCount} onChange={(lineCount) => patch({ lineCount })} format={(v) => `${v} 行`} />
        )}
        <Slider label="字幕宽度" min={20} max={70} step={1} value={prefs.widthPercent} onChange={(widthPercent) => patch({ widthPercent })} format={(v) => `${v}%`} />
        <Slider label="位置偏移" min={-17} max={20} step={0.5} value={prefs.offsetYPercent} onChange={(offsetYPercent) => patch({ offsetYPercent })} format={(v) => `${v.toFixed(1)}%`} />
        <Slider label="背景不透明" min={0} max={100} step={1} value={prefs.backgroundOpacity} onChange={(backgroundOpacity) => patch({ backgroundOpacity })} format={(v) => `${v}%`} />
        <Slider label="圆角" min={0} max={36} step={1} value={prefs.rounded} onChange={(rounded) => patch({ rounded })} format={(v) => `${v}px`} />
      </div>

      <div className="mt-5 grid gap-4 md:grid-cols-2">
        <ColorField label="字体颜色" value={prefs.textColor} onChange={(textColor) => patch({ textColor })} />
        <ColorField label="背景颜色" value={prefs.backgroundColor} onChange={(backgroundColor) => patch({ backgroundColor })} />
      </div>
    </Card>
  );
}
