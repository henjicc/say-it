import { useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Slider } from "@/components/ui/Slider";
import { cn } from "@/lib/cn";
import { syncSubtitleIndicator, toggleSubtitles } from "@/features/subtitles/controller";
import { useSubtitleStore, type SubtitleAnchor, type SubtitleMode, type SubtitleSource } from "@/store/useSubtitleStore";

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
  const { prefs, running, statusText, statusTone, latestText, patch } = useSubtitleStore();

  useEffect(() => {
    if (running) syncSubtitleIndicator(prefs);
  }, [prefs, running]);

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

      <div className="mt-5 grid gap-4 md:grid-cols-2">
        <Field label="声音来源">
          <Select
            value={prefs.source}
            disabled={running}
            onChange={(event) => patch({ source: event.target.value as SubtitleSource })}
          >
            <option value="microphone">麦克风</option>
            <option value="system">系统音频</option>
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
          <Select value={prefs.fontFamily} onChange={(event) => patch({ fontFamily: event.target.value })}>
            <option value="Microsoft YaHei">微软雅黑</option>
            <option value="SimHei">黑体</option>
            <option value="KaiTi">楷体</option>
            <option value="Segoe UI">Segoe UI</option>
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

      <div className="mt-5 grid gap-x-8 gap-y-4 md:grid-cols-2">
        <Slider label="字号" min={18} max={64} step={1} value={prefs.fontSize} onChange={(fontSize) => patch({ fontSize })} format={(v) => `${v}px`} />
        <Slider label="显示行数" min={1} max={4} step={1} value={prefs.lineCount} onChange={(lineCount) => patch({ lineCount })} format={(v) => `${v} 行`} />
        <Slider label="字幕宽度" min={420} max={1280} step={20} value={prefs.width} onChange={(width) => patch({ width })} format={(v) => `${v}px`} />
        <Slider label="位置偏移" min={-180} max={220} step={4} value={prefs.offsetY} onChange={(offsetY) => patch({ offsetY })} format={(v) => `${v}px`} />
        <Slider label="背景不透明" min={0} max={100} step={1} value={prefs.backgroundOpacity} onChange={(backgroundOpacity) => patch({ backgroundOpacity })} format={(v) => `${v}%`} />
        <Slider label="圆角" min={0} max={36} step={1} value={prefs.rounded} onChange={(rounded) => patch({ rounded })} format={(v) => `${v}px`} />
      </div>

      <div className="mt-5 grid gap-4 md:grid-cols-2">
        <ColorField label="字体颜色" value={prefs.textColor} onChange={(textColor) => patch({ textColor })} />
        <ColorField label="背景颜色" value={prefs.backgroundColor} onChange={(backgroundColor) => patch({ backgroundColor })} />
      </div>

      <div className="mt-5 rounded-xl border border-white/10 bg-white/[0.03] p-4">
        <p className="text-sm font-medium text-white">当前字幕内容</p>
        <p className="mt-2 min-h-10 whitespace-pre-wrap text-sm leading-relaxed text-white/65">
          {latestText || "开始后会显示实时识别结果"}
        </p>
      </div>
    </Card>
  );
}
