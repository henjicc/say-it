import { useEffect, useState } from "react";
import { CheckField } from "@/components/ui/Field";
import { Slider } from "@/components/ui/Slider";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { parseSubtitleSource, useSubtitleStore } from "@/store/useSubtitleStore";

const fmtMs = (value: number) => `${(value / 1000).toFixed(1)} 秒`;
const fmtThreshold = (value: number) => value.toFixed(4);
const fmtLevel = (value: number) => value.toFixed(4);
const levelWidth = (value: number) => `${Math.min(100, value * 140)}%`;

function LevelMeter({ value }: { value: number }) {
  return (
    <div className="mt-1 grid grid-cols-[7rem_1fr_3.5rem] items-center gap-3">
      <span className="text-xs text-[var(--color-fg-subtle)]">实时电平</span>
      <div className="h-2 overflow-hidden rounded-full bg-[var(--color-surface-strong)]">
        <span
          className="block h-full rounded-full bg-[var(--color-accent)] transition-[width] duration-75"
          style={{ width: levelWidth(value) }}
        />
      </div>
      <span className="text-right text-xs tabular-nums text-[var(--color-fg-subtle)]">{fmtLevel(value)}</span>
    </div>
  );
}

export function SettingsDisconnectPanel() {
  const dictPrefs = useDictPrefs((s) => s.prefs);
  const patchDictPrefs = useDictPrefs((s) => s.patch);
  const subtitlePrefs = useSubtitleStore((s) => s.prefs);
  const [dictationLevel, setDictationLevel] = useState(0);
  const [subtitleLevel, setSubtitleLevel] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    let ownsMic = false;
    const tick = async () => {
      try {
        const started = await cmd<{ reused?: boolean }>(CMD.startBackendMic, { deviceName: dictPrefs.micDeviceId || undefined });
        if (!started.reused) ownsMic = true;
        const level = await cmd<number>(CMD.getBackendMicLevel);
        if (!cancelled) setDictationLevel(level || 0);
      } catch {
        if (!cancelled) setDictationLevel(0);
      }
      if (!cancelled) timer = window.setTimeout(tick, 50);
    };
    tick();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      if (ownsMic) cmdSilent(CMD.releaseBackendMic);
    };
  }, [dictPrefs.micDeviceId]);

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    let ownsSystemAudio = false;
    const { kind, deviceName } = parseSubtitleSource(subtitlePrefs.source);
    const tick = async () => {
      try {
        if (kind === "mic") {
          const level = await cmd<number>(CMD.getBackendMicLevel);
          if (!cancelled) setSubtitleLevel(level || 0);
        } else {
          const started = await cmd<{ reused?: boolean }>(CMD.startBackendSystemAudio, { deviceName });
          if (!started.reused) ownsSystemAudio = true;
          const level = await cmd<number>(CMD.getBackendSystemAudioLevel);
          if (!cancelled) setSubtitleLevel(level || 0);
        }
      } catch {
        if (!cancelled) setSubtitleLevel(0);
      }
      if (!cancelled) timer = window.setTimeout(tick, 50);
    };
    tick();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      if (ownsSystemAudio) cmdSilent(CMD.releaseBackendSystemAudio);
    };
  }, [subtitlePrefs.source]);

  return (
    <SettingsSection title="音量长时间低于阈值就断开 ASR 流">
      <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
        开启后先本地采集检测电平，超过阈值才连接模型 API；连接后持续低于阈值达到设定时长会断开上游上传；功能保持开启，再次有声后重新连接。
      </p>
      <div className="mt-4 flex flex-col gap-5">
        <div className="grid grid-cols-1 items-start gap-3 lg:grid-cols-[9rem_minmax(12rem,1fr)_minmax(16rem,1fr)]">
          <CheckField
            checked={dictPrefs.dictationSilenceDisconnectEnabled}
            onChange={(value) => patchDictPrefs({ dictationSilenceDisconnectEnabled: value })}
          >
            语音输入
          </CheckField>
          <Slider
            label="时间"
            min={1000}
            max={30000}
            step={500}
            value={dictPrefs.dictationSilenceDisconnectMs}
            format={fmtMs}
            onChange={(value) => patchDictPrefs({ dictationSilenceDisconnectMs: value })}
          />
          <div>
            <Slider
              label="阈值"
              min={0.0001}
              max={0.1}
              step={0.0001}
              value={dictPrefs.dictationSilenceThreshold}
              format={fmtThreshold}
              onChange={(value) => patchDictPrefs({ dictationSilenceThreshold: value })}
            />
            <LevelMeter value={dictationLevel} />
          </div>
        </div>
        <div className="grid grid-cols-1 items-start gap-3 lg:grid-cols-[9rem_minmax(12rem,1fr)_minmax(16rem,1fr)]">
          <CheckField
            checked={dictPrefs.subtitleSilenceDisconnectEnabled}
            onChange={(value) => patchDictPrefs({ subtitleSilenceDisconnectEnabled: value })}
          >
            实时字幕
          </CheckField>
          <Slider
            label="时间"
            min={1000}
            max={30000}
            step={500}
            value={dictPrefs.subtitleSilenceDisconnectMs}
            format={fmtMs}
            onChange={(value) => patchDictPrefs({ subtitleSilenceDisconnectMs: value })}
          />
          <div>
            <Slider
              label="阈值"
              min={0.0001}
              max={0.1}
              step={0.0001}
              value={dictPrefs.subtitleSilenceThreshold}
              format={fmtThreshold}
              onChange={(value) => patchDictPrefs({ subtitleSilenceThreshold: value })}
            />
            <LevelMeter value={subtitleLevel} />
          </div>
        </div>
      </div>
    </SettingsSection>
  );
}
