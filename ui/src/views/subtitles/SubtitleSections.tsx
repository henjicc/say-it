import { useEffect } from "react";
import { Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";
import { Tabs } from "@/components/ui/Tabs";
import { cn } from "@/lib/cn";
import { applyObsOutputRouting, clearSubtitleShortcut, startSubtitleShortcutCapture } from "@/features/subtitles/controller";
import { useSubtitleStore, buildSubtitleSource, parseSubtitleSource, type SubtitleMode } from "@/store/useSubtitleStore";
import { useAudioDevices } from "@/features/audio/devices";
import { useDictPrefs } from "@/store/useDictPrefs";
import { SUBTITLE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { isMacOS, isWindows } from "@/lib/platform";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ModelPicker } from "@/features/models/ModelPicker";

const systemAudioSupported = isWindows || isMacOS;
const systemAudioSupportsDeviceSelection = isWindows;

export function SubtitleShortcutRow() {
  const { capturing, shortcutLabel } = useSubtitleStore();
  return (
    <Row label="快捷键" hint="用快捷键开始或停止实时字幕。">
      <span className={cn("text-[13px]", capturing ? "text-[var(--color-accent-light)]" : shortcutLabel ? "text-[var(--color-fg)]" : "text-[var(--color-fg-subtle)]")}>
        {capturing ? "请按下按键…" : shortcutLabel || "未设置"}
      </span>
      {!capturing && shortcutLabel && <Button size="sm" onClick={clearSubtitleShortcut}>清除</Button>}
      <Button size="sm" onClick={startSubtitleShortcutCapture}>{capturing ? "取消" : "修改"}</Button>
    </Row>
  );
}

/** 声音来源。麦克风设备与语音输入共用全局偏好，这里只决定用麦克风还是系统音频。 */
export function SubtitleSourceRow() {
  const { prefs, running, patch } = useSubtitleStore();
  const { inputs, outputs } = useAudioDevices();
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);
  const dictPatch = useDictPrefs((s) => s.patch);
  const parsedSource = parseSubtitleSource(prefs.source);
  useEffect(() => {
    if (!systemAudioSupported && parsedSource.kind === "system") {
      patch({ source: buildSubtitleSource("mic") });
    }
  }, [parsedSource.kind, patch]);
  const value = parsedSource.kind === "mic" ? buildSubtitleSource("mic", micDeviceId || undefined) : prefs.source;
  const onChange = (nextValue: string) => {
    const next = parseSubtitleSource(nextValue);
    if (next.kind === "mic") dictPatch({ micDeviceId: next.deviceName || "" });
    patch({ source: nextValue });
  };

  return (
    <Row label="声音来源">
      <Select
        className={ROW_CONTROL_W}
        aria-label="声音来源"
        searchable={inputs.length + outputs.length > 5}
        searchPlaceholder="搜索设备…"
        value={value}
        disabled={running}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value={buildSubtitleSource("mic")}>麦克风（系统默认）</option>
        <option value={buildSubtitleSource("system")} disabled={!systemAudioSupported}>
          {systemAudioSupported ? (isMacOS ? "系统音频（需录制权限）" : "系统音频（默认）") : "系统音频（当前平台暂不支持）"}
        </option>
        {inputs.length > 0 && <option value="__group_inputs" disabled>— 输入设备 —</option>}
        {inputs.map((device) => (
          <option key={`in:${device.name}`} value={buildSubtitleSource("mic", device.name)}>{device.name}</option>
        ))}
        {systemAudioSupportsDeviceSelection && outputs.length > 0 && <option value="__group_outputs" disabled>— 输出设备 —</option>}
        {systemAudioSupportsDeviceSelection && outputs.map((device) => (
          <option key={`out:${device.name}`} value={buildSubtitleSource("system", device.name)}>{device.name}</option>
        ))}
      </Select>
    </Row>
  );
}

export function SubtitleAsrRow() {
  useModelCatalogRevision();
  const { prefs, running, patch } = useSubtitleStore();
  return (
    <Row label="识别模型" controlId="subtitle-asr-model">
      <div className={ROW_CONTROL_W}>
        <ModelPicker
          id="subtitle-asr-model"
          value={prefs.asrModel}
          disabled={running}
          options={SUBTITLE_ASR_MODEL_OPTIONS}
          panelLabel="选择语音识别模型"
          onChange={(value) => patch({ asrModel: value })}
        />
      </div>
    </Row>
  );
}

export function SubtitleModeRow() {
  const { prefs, patch } = useSubtitleStore();
  return (
    <Row label="更新方式" hint="单句替换：一句话显示完再换下一句；滚动累积：新内容接在后面滚动。">
      <Tabs
        variant="view"
        ariaLabel="字幕更新方式"
        tabs={[{ key: "replace", label: "单句替换" }, { key: "scroll", label: "滚动累积" }]}
        active={prefs.mode}
        onChange={(mode) => patch({ mode: mode as SubtitleMode })}
      />
    </Row>
  );
}

export function SubtitleOutputRow() {
  const { prefs, patch } = useSubtitleStore();
  return (
    <Row label="显示在" hint="输出到 OBS 前需要先连接 OBS 并安装字幕源；OBS 未就绪时会自动回到桌面。">
      <Tabs
        variant="view"
        ariaLabel="字幕显示位置"
        tabs={[{ key: "desktop", label: "桌面" }, { key: "obs", label: "OBS" }]}
        active={prefs.obsOutputEnabled ? "obs" : "desktop"}
        onChange={(key) => {
          patch({ obsOutputEnabled: key === "obs" });
          void applyObsOutputRouting();
        }}
      />
    </Row>
  );
}
