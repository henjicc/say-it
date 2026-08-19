import { useEffect, useMemo, useRef, useState } from "react";
import {
  CheckCircle2,
  ChevronLeft,
  CircleAlert,
  Gauge,
  Keyboard,
  LoaderCircle,
  Mic2,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Input } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { CMD, cmd, type SetupCheckResult, type SetupStatus } from "@/lib/tauri";
import { isMacOS } from "@/lib/platform";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useUiStore } from "@/store/useUiStore";

const STEPS = [
  { title: "欢迎", description: "了解首次设置会完成什么" },
  { title: "识别方式", description: "确认本地模型或云服务" },
  { title: isMacOS ? "麦克风与权限" : "麦克风", description: "检查实际听写输入" },
  { title: "快捷键与输入", description: "完成日常使用设置" },
] as const;

function StatusIcon({ item, running }: { item: SetupCheckResult; running: boolean }) {
  if (running) {
    return <LoaderCircle className="h-5 w-5 shrink-0 animate-spin text-[var(--color-accent)]" aria-hidden />;
  }
  if (item.status === "ready") {
    return <CheckCircle2 className="h-5 w-5 shrink-0 text-[var(--color-ok)]" aria-hidden />;
  }
  return <CircleAlert className="h-5 w-5 shrink-0 text-[var(--color-warn)]" aria-hidden />;
}

function CheckRow({
  item,
  running,
  actionLabel,
  onAction,
  onCheck,
}: {
  item?: SetupCheckResult;
  running: boolean;
  actionLabel?: string;
  onAction?: () => void;
  onCheck: (item: SetupCheckResult) => void;
}) {
  if (!item) return null;
  return (
    <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
      <StatusIcon item={item} running={running} />
      <div className="min-w-0 flex-1">
        <strong className="text-sm text-[var(--color-fg)]">{item.title}</strong>
        <p className="mt-1 text-xs leading-5 text-[var(--color-fg-subtle)]">{item.message}</p>
      </div>
      {onAction && <Button size="sm" className="shrink-0 whitespace-nowrap" onClick={onAction}>{actionLabel}</Button>}
      <Button size="sm" className="shrink-0 whitespace-nowrap" onClick={() => onCheck(item)} disabled={running}>重查</Button>
    </div>
  );
}

export function OnboardingWizard({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [step, setStep] = useState(0);
  const [running, setRunning] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [micLevel, setMicLevel] = useState(0);
  const testInput = useRef<HTMLInputElement>(null);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);
  const micDeviceId = useDictPrefs((state) => state.prefs.micDeviceId);

  const checks = useMemo(
    () => Object.fromEntries((status?.checks ?? []).map((item) => [item.id, item])) as Partial<Record<SetupCheckResult["id"], SetupCheckResult>>,
    [status],
  );
  const displayLevel = Math.min(1, Math.sqrt(Math.max(0, micLevel)));

  async function refresh() {
    setStatus(await cmd<SetupStatus>(CMD.getSetupStatus));
  }

  useEffect(() => {
    if (!open) return;
    setStep(0);
    setMessage("");
    void refresh().catch((error) => setMessage(String(error)));
  }, [open]);

  useEffect(() => {
    if (!open || step !== 2) return;
    let cancelled = false;
    let ownsMic = false;
    let meterStarted = false;
    let timer: ReturnType<typeof setInterval> | undefined;

    void cmd<{ reused?: boolean }>(CMD.startBackendMic, { deviceName: micDeviceId || undefined })
      .then(async (result) => {
        ownsMic = !result.reused;
        if (cancelled) {
          if (ownsMic) await cmd(CMD.releaseBackendMic).catch(() => undefined);
          return;
        }
        await cmd(CMD.startSetupMicMeter);
        meterStarted = true;
        if (cancelled) {
          await cmd(CMD.stopSetupMicMeter).catch(() => undefined);
          return;
        }
        timer = setInterval(() => {
          void cmd<number>(CMD.getSetupMicLevel)
            .then((level) => setMicLevel(Math.max(0, Math.min(1, level))))
            .catch(() => undefined);
        }, 120);
      })
      .catch((error) => setMessage(`麦克风检测未启动：${String(error)}`));

    return () => {
      cancelled = true;
      setMicLevel(0);
      if (timer) clearInterval(timer);
      if (meterStarted) void cmd(CMD.stopSetupMicMeter).catch(() => undefined);
      if (ownsMic) void cmd(CMD.releaseBackendMic).catch(() => undefined);
    };
  }, [micDeviceId, open, step]);

  async function run(item: SetupCheckResult) {
    setRunning(item.id);
    setMessage("");
    try {
      const next = await cmd<SetupCheckResult>(
        item.id === "permissions" ? CMD.requestSetupPermissions : CMD.runSetupCheck,
        item.id === "permissions" ? undefined : { id: item.id },
      );
      setStatus((current) => current
        ? { ...current, checks: current.checks.map((check) => check.id === next.id ? next : check) }
        : current);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  function openSettings(tab: "model" | "audio" | "keys") {
    setView("settings");
    setSettingsTab(tab);
    onClose();
  }

  async function testInjection() {
    testInput.current?.focus();
    setRunning("injection");
    setMessage("");
    try {
      await cmd(CMD.runInjectionSetupCheck, { text: "说吧！注入测试成功" });
      setMessage("测试文字已发送；输入框出现文字就说明输入链路正常。");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  async function finish() {
    setRunning("finish");
    try {
      await cmd(CMD.completeOnboarding);
      onClose();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title="首次使用引导" className="max-w-[720px]">
      <div className="flex min-h-[480px] flex-col">
        <div className="border-b border-[var(--color-line)] px-6 py-4">
          <div className="flex items-center justify-between gap-4">
            <div>
              <p className="text-sm font-semibold text-[var(--color-fg)]">{STEPS[step].title}</p>
              <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">{STEPS[step].description}</p>
            </div>
            <span className="text-xs tabular-nums text-[var(--color-fg-subtle)]">{step + 1} / {STEPS.length}</span>
          </div>
          <div className="mt-3 grid grid-cols-4 gap-2" aria-label={`引导进度：第 ${step + 1} 步，共 ${STEPS.length} 步`}>
            {STEPS.map((item, index) => (
              <div key={item.title} className={`h-1 rounded-[var(--radius-pill)] ${index <= step ? "bg-[var(--color-accent)]" : "bg-[var(--color-surface-strong)]"}`} />
            ))}
          </div>
        </div>

        <div className="flex flex-1 flex-col gap-4 px-6 py-5">
          {step === 0 && <>
            <div className="flex items-start gap-4 py-2">
              <div className="grid h-12 w-12 shrink-0 place-items-center rounded-[var(--radius-lg)] bg-[var(--accent-soft)] text-[var(--color-accent-light)]">
                <Sparkles className="h-6 w-6" aria-hidden />
              </div>
              <div>
                <h2 className="text-xl font-semibold text-[var(--color-fg)]">几步完成，说完就能输入</h2>
                <p className="mt-2 max-w-[58ch] text-sm leading-6 text-[var(--color-fg-muted)]">接下来会确认识别方式、麦克风和快捷键。检查都在本机完成，不会上传测试内容，也不会替你选择或启用云服务。</p>
              </div>
            </div>
            <div className="mt-3 grid grid-cols-3 gap-3">
              {[
                [Mic2, "选择识别方式", "使用本地模型，或配置自己的云服务密钥"],
                [Gauge, "试听实际输入", "显示经过当前降噪与音频调校后的音量"],
                [Keyboard, "确认快捷键", "最后测试文字能否输入到其他软件"],
              ].map(([Icon, title, text]) => (
                <div key={String(title)} className="rounded-[var(--radius-lg)] border border-[var(--color-line)] p-4">
                  <Icon className="h-5 w-5 text-[var(--color-accent-light)]" aria-hidden />
                  <strong className="mt-3 block text-sm text-[var(--color-fg)]">{title as string}</strong>
                  <p className="mt-1 text-xs leading-5 text-[var(--color-fg-subtle)]">{text as string}</p>
                </div>
              ))}
            </div>
          </>}

          {step === 1 && <>
            <div>
              <h2 className="text-lg font-semibold text-[var(--color-fg)]">先准备一种语音识别方式</h2>
              <p className="mt-2 text-sm leading-6 text-[var(--color-fg-muted)]">本地模型无需密钥；云服务需要配置你自己的 API Key。说吧！只检查当前是否可用，不会自动替你选默认服务。</p>
            </div>
            <CheckRow
              item={checks.provider}
              running={running === "provider"}
              actionLabel="配置识别方式"
              onAction={() => openSettings("model")}
              onCheck={(item) => void run(item)}
            />
            {checks.provider?.status !== "ready" && (
              <p className="text-xs leading-5 text-[var(--color-fg-subtle)]">配置完成后可从“设置 → 通用 → 使用引导”回到这里继续。</p>
            )}
          </>}

          {step === 2 && <>
            <div>
              <h2 className="text-lg font-semibold text-[var(--color-fg)]">说句话，确认实际输入音量</h2>
              <p className="mt-2 text-sm leading-6 text-[var(--color-fg-muted)]">下面显示的是经过当前降噪、均衡和响度调校后的音量，和听写链路使用的声音一致。</p>
            </div>
            <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
              <div className="flex items-center gap-3" aria-label={`处理后麦克风音量 ${Math.round(displayLevel * 100)}%`}>
                <span className="w-24 shrink-0 text-xs font-medium text-[var(--color-fg-muted)]">处理后音量</span>
                <div className="h-2 flex-1 overflow-hidden rounded-[var(--radius-pill)] bg-[var(--color-surface-strong)]">
                  <div className="h-full rounded-[var(--radius-pill)] bg-[var(--color-accent)] transition-[width] duration-[var(--dur-fast)] motion-reduce:transition-none" style={{ width: `${displayLevel * 100}%` }} />
                </div>
                <span className="w-10 text-right text-xs tabular-nums text-[var(--color-fg-subtle)]">{Math.round(displayLevel * 100)}%</span>
              </div>
            </div>
            <CheckRow
              item={checks.microphone}
              running={running === "microphone"}
              actionLabel={checks.microphone?.action ? "音频设置" : undefined}
              onAction={checks.microphone?.action ? () => openSettings("audio") : undefined}
              onCheck={(item) => void run(item)}
            />
            {checks.permissions && <CheckRow
              item={checks.permissions}
              running={running === "permissions"}
              actionLabel={checks.permissions.action ? "授予权限" : undefined}
              onAction={checks.permissions.action ? () => void run(checks.permissions!) : undefined}
              onCheck={(item) => void run(item)}
            />}
            {isMacOS && <p className="flex items-start gap-2 text-xs leading-5 text-[var(--color-fg-subtle)]"><ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />基础听写需要辅助功能权限；只有启用窗口 OCR 时才需要屏幕录制权限。</p>}
          </>}

          {step === 3 && <>
            <div>
              <h2 className="text-lg font-semibold text-[var(--color-fg)]">确认快捷键和文字输入</h2>
              <p className="mt-2 text-sm leading-6 text-[var(--color-fg-muted)]">使用主快捷键开始或结束听写，再用一次本地测试确认识别结果能送到当前输入框。</p>
            </div>
            <CheckRow
              item={checks.shortcut}
              running={running === "shortcut"}
              actionLabel={checks.shortcut?.action ? "快捷键设置" : undefined}
              onAction={checks.shortcut?.action ? () => openSettings("keys") : undefined}
              onCheck={(item) => void run(item)}
            />
            <Field
              label="文字输入测试"
              controlId="setup-injection-test"
              hint="点击测试后，输入框出现“说吧！注入测试成功”即表示输入链路正常。"
              actions={<Button onClick={() => void testInjection()} disabled={running !== null}>测试</Button>}
            >
              <Input ref={testInput} id="setup-injection-test" placeholder="测试文字会出现在这里" />
            </Field>
          </>}

          {message && <p role="status" className="mt-auto text-xs leading-5 text-[var(--color-fg-subtle)]">{message}</p>}
        </div>

        <div className="flex items-center justify-between border-t border-[var(--color-line)] px-6 py-4">
          <Button onClick={() => setStep((current) => Math.max(0, current - 1))} disabled={step === 0 || running !== null}>
            <ChevronLeft className="h-4 w-4" aria-hidden />上一步
          </Button>
          {step < STEPS.length - 1
            ? <Button variant="primary" onClick={() => setStep((current) => Math.min(STEPS.length - 1, current + 1))} disabled={running !== null}>{step === 0 ? "开始设置" : "下一步"}</Button>
            : <Button variant="primary" onClick={() => void finish()} disabled={running !== null}>{running === "finish" ? "正在保存…" : "完成引导"}</Button>}
        </div>
      </div>
    </Modal>
  );
}
