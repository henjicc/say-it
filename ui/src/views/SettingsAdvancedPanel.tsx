import { useEffect, useRef, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Slider } from "@/components/ui/Slider";
import { CheckField, Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import { cn } from "@/lib/cn";
import { CMD, cmd, cmdSilent, type AppSnapshot, type DiagnosticStatus } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useAudioStore } from "@/store/useAudioStore";
import { parseSubtitleSource, useSubtitleStore } from "@/store/useSubtitleStore";
import { dspDefaults } from "@/lib/audio-dsp";
import * as lab from "@/features/audio/lab";

const toneClass: Record<string, string> = {
  "": "text-[var(--color-fg-subtle)]",
  ok: "text-[var(--color-ok)]",
  err: "text-[var(--color-err)]",
};

const fmtGainDb = (v: number) => `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`;

const fmt = {
  targetLufs: (v: number) => `${v.toFixed(1)} LUFS`,
  maxGainDb: (v: number) => `${v.toFixed(1)} dB`,
  peakLimitDbfs: (v: number) => `${v.toFixed(1)} dB`,
  denoiseStrength: (v: number) => `${Math.round(v * 100)}%`,
  vadGate: (v: number) => (v <= 0 ? "关闭" : v.toFixed(2)),
  bassGainDb: fmtGainDb,
  trebleGainDb: fmtGainDb,
};

const fmtMs = (value: number) => `${(value / 1000).toFixed(1)} 秒`;
const fmtThreshold = (value: number) => value.toFixed(4);
const levelWidth = (value: number) => `${Math.min(100, value * 140)}%`;

export function DiagnosticSection() {
  const [verboseLogging, setVerboseLogging] = useState(false);
  const [status, setStatus] = useState<DiagnosticStatus | null>(null);
  const [includeContent, setIncludeContent] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    void Promise.all([
      cmd<AppSnapshot>(CMD.getAppSnapshot),
      cmd<DiagnosticStatus>(CMD.getDiagnosticStatus),
    ]).then(([snapshot, nextStatus]) => {
      setVerboseLogging(snapshot.settings.diagnosticsPrefs.verboseLogging === true);
      setStatus(nextStatus);
    }).catch((error) => setMessage(String(error)));
  }, []);

  useEffect(() => {
    if (!status?.contentLoggingEnabled) return;
    const timer = window.setInterval(() => {
      setStatus((current) => current ? {
        ...current,
        contentLoggingRemainingSeconds: Math.max(0, current.contentLoggingRemainingSeconds - 1),
        contentLoggingEnabled: current.contentLoggingRemainingSeconds > 1,
      } : current);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [status?.contentLoggingEnabled]);

  async function updateVerbose(enabled: boolean) {
    setVerboseLogging(enabled);
    try {
      await cmd(CMD.updateAppSettings, { domain: "diagnostics", value: { verboseLogging: enabled } });
    } catch (error) {
      setVerboseLogging(!enabled);
      setMessage(String(error));
    }
  }

  async function updateContent(enabled: boolean) {
    if (enabled && !window.confirm("临时正文日志会记录输入文本，可能包含隐私内容。确定开启 30 分钟吗？")) return;
    try {
      setStatus(await cmd<DiagnosticStatus>(CMD.setContentDiagnostics, { enabled }));
      setMessage(enabled ? "正文日志已开启，将在 30 分钟后自动关闭" : "正文日志已关闭");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function clearLogs() {
    if (!window.confirm("确定清空全部诊断日志（包括正文日志）吗？")) return;
    try {
      await cmd(CMD.clearDiagnosticLogs);
      setIncludeContent(false);
      setStatus(await cmd<DiagnosticStatus>(CMD.getDiagnosticStatus));
      setMessage("诊断日志已清空");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function exportBundle() {
    const destination = await saveDialog({
      defaultPath: "say-it-diagnostics.zip",
      filters: [{ name: "ZIP 压缩包", extensions: ["zip"] }],
    });
    if (!destination) return;
    try {
      await cmd(CMD.exportDiagnosticBundle, { destination, includeContent });
      setMessage(includeContent ? "诊断包已导出，其中包含输入文本" : "诊断包已导出（不含正文）");
    } catch (error) {
      setMessage(String(error));
    }
  }

  return (
    <SettingsSection title="诊断日志">
      <FormGrid>
        <Field label="详细元数据日志" controlId="diagnostic-verbose-logging">
          <Switch id="diagnostic-verbose-logging" checked={verboseLogging} onChange={(enabled) => void updateVerbose(enabled)} label="详细元数据日志" />
        </Field>
        <Field
          label="临时正文日志"
          controlId="diagnostic-content-logging"
          hint={status?.contentLoggingEnabled
            ? `包含输入文本，将在 ${Math.ceil(status.contentLoggingRemainingSeconds / 60)} 分钟内自动关闭。`
            : "默认关闭；开启后记录输入文本，30 分钟后自动关闭，重启不会恢复。"}
        >
          <Switch id="diagnostic-content-logging" checked={status?.contentLoggingEnabled === true} onChange={(enabled) => void updateContent(enabled)} label="临时正文日志" />
        </Field>
        <Field label="日志目录" controlId="diagnostic-open-directory">
          <Button id="diagnostic-open-directory" onClick={() => void cmd(CMD.openDiagnosticDirectory).catch((error) => setMessage(String(error)))}>打开日志目录</Button>
        </Field>
        <Field label="清空日志" controlId="diagnostic-clear-logs">
          <Button id="diagnostic-clear-logs" variant="dangerHover" onClick={() => void clearLogs()}>清空诊断日志</Button>
        </Field>
        <Field
          label="导出诊断包"
          controlId="diagnostic-export-bundle"
          hint="默认仅包含版本、平台、非敏感配置投影和普通日志，不包含历史数据库、凭据、音频或截图。"
          className="sm:col-span-2"
        >
          <div className="flex flex-wrap items-center gap-3">
            <Button id="diagnostic-export-bundle" onClick={() => void exportBundle()}>导出诊断包</Button>
            <CheckField checked={includeContent} onChange={setIncludeContent}>包含正文日志</CheckField>
            {includeContent && <span role="alert" className="text-xs text-[var(--color-err)]">风险：导出包包含输入文本</span>}
          </div>
        </Field>
      </FormGrid>
      {message && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </SettingsSection>
  );
}

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
      <span className="text-right text-xs tabular-nums text-[var(--color-fg-subtle)]">{value.toFixed(4)}</span>
    </div>
  );
}

function SilenceDisconnectSection() {
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
        if (cancelled) {
          if (!started.reused) cmdSilent(CMD.releaseBackendMic);
          return;
        }
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
          if (cancelled) {
            if (!started.reused) cmdSilent(CMD.releaseBackendSystemAudio);
            return;
          }
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
    <SettingsSection title="静音断流">
      <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
        开启后先本地采集检测电平，超过阈值才连接模型 API；连接后持续低于阈值达到设定时长会断开上游上传；功能保持开启，再次有声后重新连接。
      </p>
      <div className="flex flex-col gap-5">
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

function AudioLabSections() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const { recording, recInfo, recTone, canPlay, meters, labStatus, labStatusTone } = useAudioStore();
  const origRef = useRef<HTMLCanvasElement>(null);
  const procRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    lab.setCanvases(origRef.current, procRef.current);
    return () => lab.setCanvases(null, null);
  }, []);

  const onParam = (key: keyof typeof fmt, value: number) => {
    patch({ [key]: value });
    lab.paramChanged();
  };

  const reset = () => {
    patch({ ...dspDefaults });
    lab.resetParams();
  };

  return (
    <>
      <SettingsSection title="响度与降噪">
        <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
          处理算法与实际语音输入共用 Rust DSP：RNNoise 降噪 + LUFS 响度归一化，调好后自动应用到语音输入。
        </p>
        <div className="grid grid-cols-1 gap-8 sm:grid-cols-2">
          <div className="flex flex-col gap-3">
            <h3 className="text-sm font-semibold text-[var(--color-fg-muted)]">响度归一化</h3>
            <Slider label="目标响度" min={-30} max={-14} step={0.5} value={prefs.targetLufs} format={fmt.targetLufs} onChange={(v) => onParam("targetLufs", v)} />
            <Slider label="最大提升" min={0} max={80} step={1} value={prefs.maxGainDb} format={fmt.maxGainDb} onChange={(v) => onParam("maxGainDb", v)} />
            <Slider label="峰值上限" min={-6} max={-0.5} step={0.5} value={prefs.peakLimitDbfs} format={fmt.peakLimitDbfs} onChange={(v) => onParam("peakLimitDbfs", v)} />
            <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
              建议语音目标先用 -20 LUFS；如果希望更响可试 -18 LUFS。最大提升用于防止把近似静音的底噪硬拉上来。
            </p>
          </div>
          <div className="flex flex-col gap-3">
            <h3 className="text-sm font-semibold text-[var(--color-fg-muted)]">RNNoise 降噪</h3>
            <CheckField
              checked={prefs.denoiseEnabled}
              onChange={(v) => {
                patch({ denoiseEnabled: v });
                lab.paramChanged();
              }}
            >
              启用降噪
            </CheckField>
            <Slider label="降噪强度" min={0} max={1} step={0.05} value={prefs.denoiseStrength} format={fmt.denoiseStrength} onChange={(v) => onParam("denoiseStrength", v)} />
            <Slider label="VAD 静音门" min={0} max={0.9} step={0.05} value={prefs.vadGate} format={fmt.vadGate} onChange={(v) => onParam("vadGate", v)} />
            <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
              降噪强度 100% 是完整 RNNoise 输出；如果声音发闷可降到 70%~85%。VAD 静音门默认关闭，只有停顿底噪特别明显时再小幅打开。
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <Button size="sm" onClick={reset}>
            恢复默认
          </Button>
          <span className={cn("text-xs", toneClass[labStatusTone])}>{labStatus}</span>
        </div>
      </SettingsSection>

      <SettingsSection title="均衡器（高低频）">
        <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
          两段搁架 EQ：低频拐点约 150Hz、高频拐点约 4000Hz，分别调整声音的"厚度"和"亮度"。0 dB 为不调整。
        </p>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Slider label="低频增益" min={-12} max={12} step={0.5} value={prefs.bassGainDb} format={fmt.bassGainDb} onChange={(v) => onParam("bassGainDb", v)} />
          <Slider label="高频增益" min={-12} max={12} step={0.5} value={prefs.trebleGainDb} format={fmt.trebleGainDb} onChange={(v) => onParam("trebleGainDb", v)} />
        </div>
      </SettingsSection>

      <SettingsSection title="录音试听与波形">
        <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
          录一段话 → 调上面的参数 → A/B 试听「原始 vs 处理后」。
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant={recording ? "danger" : "primary"} onClick={lab.toggleRecord}>
            {recording ? "■ 停止录音" : "● 开始录音"}
          </Button>
          <Button disabled={!canPlay} onClick={lab.playOriginal}>
            ▶ 播放原始
          </Button>
          <Button disabled={!canPlay} onClick={lab.playProcessed}>
            ▶ 播放处理后
          </Button>
          {recInfo && <span className={cn("text-xs", toneClass[recTone])}>{recInfo}</span>}
        </div>
        <div className="grid grid-cols-1 gap-2 text-xs text-[var(--color-fg-muted)] sm:grid-cols-3">
          <div>原始：LUFS <b className="text-[var(--color-fg)]">{meters.olufs}</b>｜RMS <b className="text-[var(--color-fg)]">{meters.orms}</b> dB｜峰值 <b className="text-[var(--color-fg)]">{meters.opeak}</b> dB</div>
          <div>处理后：LUFS <b className="text-[var(--color-fg)]">{meters.plufs}</b>｜RMS <b className="text-[var(--color-fg)]">{meters.prms}</b> dB｜峰值 <b className="text-[var(--color-fg)]">{meters.ppeak}</b> dB</div>
          <div>削波样本：<b className="text-[var(--color-fg)]">{meters.clip}</b></div>
        </div>
        <div>
          <div className="text-xs text-[var(--color-fg-subtle)]">原始波形</div>
          <canvas ref={origRef} width={860} height={90} className="mt-1 w-full rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]" />
        </div>
        <div>
          <div className="text-xs text-[var(--color-fg-subtle)]">处理后波形（增益 + 降噪）</div>
          <canvas ref={procRef} width={860} height={90} className="mt-1 w-full rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]" />
        </div>
      </SettingsSection>
    </>
  );
}

export function SettingsAdvancedPanel() {
  return (
    <div className="flex flex-col gap-8">
      <SilenceDisconnectSection />
      <AudioLabSections />
      <DiagnosticSection />
    </div>
  );
}
