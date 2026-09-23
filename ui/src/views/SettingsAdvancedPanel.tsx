import { HelpLabel } from "@/components/ui/Tooltip";
import { useEffect, useRef, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Slider } from "@/components/ui/Slider";
import { CheckField, Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { Modal } from "@/components/ui/Modal";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { useConfirm } from "@/components/ui/useConfirm";
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
  const { confirm, dialog } = useConfirm();
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
    if (
      enabled &&
      !(await confirm({
        title: "开启临时正文日志",
        message: "临时正文日志会记录输入文本，可能包含隐私内容。确定开启 30 分钟吗？",
        confirmLabel: "开启 30 分钟",
        danger: false,
      }))
    ) {
      return;
    }
    try {
      setStatus(await cmd<DiagnosticStatus>(CMD.setContentDiagnostics, { enabled }));
      setMessage(enabled ? "正文日志已开启，将在 30 分钟后自动关闭" : "正文日志已关闭");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function clearLogs() {
    if (
      !(await confirm({
        title: "确认清空诊断日志",
        message: "将清空全部诊断日志，包括正文日志。此操作不可撤销。",
        confirmLabel: "清空日志",
      }))
    ) {
      return;
    }
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
          hint="默认仅包含版本、平台、不含隐私的设置和运行日志，不包含历史记录、密钥、音频或截图。"
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
      {dialog}
    </SettingsSection>
  );
}

export function DataResetSection() {
  const [pendingReset, setPendingReset] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  // 失败提示必须留在弹窗**里面**。
  //
  // 原本写在弹窗外面的状态行上，而失败时 pendingReset 不会被清掉：弹窗还开着、
  // 遮罩把那行字整个挡住，用户按了「确认重置」之后只看到按钮从「正在重置…」跳回
  // 「确认重置」，得不到任何失败原因。
  function closeDialog() {
    if (busy) return;
    setPendingReset(false);
    setMessage("");
  }

  async function reset() {
    setBusy(true);
    setMessage("");
    try {
      await cmd(CMD.requestDataReset);
    } catch (error) {
      setBusy(false);
      setMessage(String(error));
    }
  }

  return (
    <SettingsSection title="重置数据">
      <p className="text-xs leading-relaxed text-[var(--color-fg-subtle)]">
        清空设置、历史记录、学习记忆、已安装插件和本地模型等全部本地数据，恢复到刚安装时的状态；已保存的 API 密钥/凭据不受影响。重置后应用会立即重启。
      </p>
      <FormGrid>
        <Field label="重置全部数据" controlId="data-reset" message="不可撤销，请谨慎操作。">
          <Button id="data-reset" variant="dangerHover" onClick={() => setPendingReset(true)}>重置数据并重启</Button>
        </Field>
      </FormGrid>
      <Modal
        open={pendingReset}
        onClose={closeDialog}
        title="确认重置全部数据"
        showCloseButton={false}
        className="max-w-[430px]"
      >
        <div className="p-5">
          <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            将清空设置、历史记录、学习记忆、已安装插件和本地模型等全部本地数据，恢复到刚安装时的状态；已保存的 API 密钥/凭据不受影响。此操作不可撤销，确认后应用会立即重启。
          </p>
          {message && (
            <p role="alert" className="mt-4 text-sm text-[var(--color-err)]">
              重置失败：{message}
            </p>
          )}
          <div className="mt-6 flex justify-end gap-2">
            <Button size="sm" variant="dangerHover" disabled={busy} onClick={() => void reset()}>
              {busy ? "正在重置…" : "确认重置"}
            </Button>
            <Button size="sm" variant="primary" autoFocus disabled={busy} onClick={closeDialog}>取消</Button>
          </div>
        </div>
      </Modal>
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
    <SettingsSection title="静音断流" description="开启后，有声音时才连接识别服务；持续安静达到设定时间后会暂停上传。再次有声音时自动恢复。">
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
      <SettingsSection title="响度与降噪" description="在这里调整降噪和音量，设置会自动用于之后的语音输入。">
        <div className="grid grid-cols-1 gap-8 sm:grid-cols-2">
          <div className="flex flex-col gap-3">
            <h3 className="text-sm font-semibold text-[var(--color-fg-muted)]"><HelpLabel content="让音量更均匀。目标响度建议先用 -20，想更响可试 -18。“最大提升”限制音量放大幅度，避免同时放大背景噪声。">响度归一化</HelpLabel></h3>
            <Slider label="目标响度" min={-30} max={-14} step={0.5} value={prefs.targetLufs} format={fmt.targetLufs} onChange={(v) => onParam("targetLufs", v)} />
            <Slider label="最大提升" min={0} max={80} step={1} value={prefs.maxGainDb} format={fmt.maxGainDb} onChange={(v) => onParam("maxGainDb", v)} />
            <Slider label="峰值上限" min={-6} max={-0.5} step={0.5} value={prefs.peakLimitDbfs} format={fmt.peakLimitDbfs} onChange={(v) => onParam("peakLimitDbfs", v)} />
          </div>
          <div className="flex flex-col gap-3">
            <h3 className="text-sm font-semibold text-[var(--color-fg-muted)]"><HelpLabel content="100% 为最强降噪。声音发闷时可降到 70%～85%；停顿时仍有明显噪声，再适当提高“静音抑制”。">人声降噪</HelpLabel></h3>
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
            <Slider label="静音抑制" min={0} max={0.9} step={0.05} value={prefs.vadGate} format={fmt.vadGate} onChange={(v) => onParam("vadGate", v)} />
          </div>
        </div>
        <div className="flex items-center gap-3">
          <Button size="sm" onClick={reset}>
            恢复默认
          </Button>
          <span className={cn("text-xs", toneClass[labStatusTone])}>{labStatus}</span>
        </div>
      </SettingsSection>

      <SettingsSection title="均衡器（高低频）" description="低频影响声音的厚实程度，高频影响明亮程度。数值为 0 时保持原样。">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Slider label="低频增益" min={-12} max={12} step={0.5} value={prefs.bassGainDb} format={fmt.bassGainDb} onChange={(v) => onParam("bassGainDb", v)} />
          <Slider label="高频增益" min={-12} max={12} step={0.5} value={prefs.trebleGainDb} format={fmt.trebleGainDb} onChange={(v) => onParam("trebleGainDb", v)} />
        </div>
      </SettingsSection>

      <SettingsSection title="录音试听与波形" description="先录一段话，再调整参数，分别播放原始和处理后的声音进行比较。">
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
      <DataResetSection />
    </div>
  );
}
