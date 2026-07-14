import { useEffect } from "react";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { ClearIcon } from "@/components/ui/icons";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { FormGrid } from "@/components/ui/FormGrid";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { startSubtitleShortcutCapture, clearSubtitleShortcut } from "@/features/subtitles/controller";
import { useSubtitleStore, buildSubtitleSource, parseSubtitleSource, type SubtitleMode } from "@/store/useSubtitleStore";
import { useAudioDevices } from "@/features/audio/devices";
import { useDictPrefs } from "@/store/useDictPrefs";
import { SUBTITLE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";

const shortcutActionButtonClassName = "min-h-[var(--control-h)] shrink-0 self-stretch";
const systemAudioSupported = !navigator.userAgent.includes("Macintosh");

export function SubtitleGeneralPanel() {
  const { prefs, running, capturing, shortcutLabel, patch } = useSubtitleStore();
  const { inputs, outputs } = useAudioDevices();
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const dictPatch = useDictPrefs((s) => s.patch);

  // 麦克风设备是语音输入和实时字幕共用的全局偏好（同一个后端采集单例），
  // 这里下拉框选中的"麦克风"具体设备始终跟着这个全局值走，而不是自己单独存一份。
  const parsedSource = parseSubtitleSource(prefs.source);
  useEffect(() => {
    if (!systemAudioSupported && parsedSource.kind === "system") {
      patch({ source: buildSubtitleSource("mic") });
    }
  }, [parsedSource.kind, patch]);
  const sourceSelectValue =
    parsedSource.kind === "mic" ? buildSubtitleSource("mic", micDeviceId || undefined) : prefs.source;
  const onSourceChange = (nextValue: string) => {
    const next = parseSubtitleSource(nextValue);
    if (next.kind === "mic") dictPatch({ micDeviceId: next.deviceName || "" });
    patch({ source: nextValue });
  };

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
              <option value={buildSubtitleSource("system")} disabled={!systemAudioSupported}>
                {systemAudioSupported ? "系统音频（默认）" : "系统音频（macOS 暂不支持）"}
              </option>
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
              {systemAudioSupported && outputs.length > 0 && (
                <option value="__group_outputs" disabled>
                  — 输出设备 —
                </option>
              )}
              {systemAudioSupported && outputs.map((device) => (
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
        </FormGrid>
      </SettingsSection>
    </div>
  );
}
