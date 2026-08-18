import { useEffect, useRef, useState } from "react";
import { CheckCircle2, CircleAlert, LoaderCircle, Stethoscope } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { CMD, cmd, type SetupCheckResult, type SetupStatus } from "@/lib/tauri";
import { useUiStore } from "@/store/useUiStore";

export function OnboardingWizard({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [micLevel, setMicLevel] = useState(0);
  const testInput = useRef<HTMLInputElement>(null);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);

  async function refresh() {
    setStatus(await cmd<SetupStatus>(CMD.getSetupStatus));
  }

  useEffect(() => {
    if (open) void refresh().catch((error) => setMessage(String(error)));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    let ownsMic = false;
    let timer: ReturnType<typeof setInterval> | undefined;
    void cmd<{ reused?: boolean }>(CMD.startBackendMic)
      .then((result) => {
        if (cancelled) return;
        ownsMic = !result.reused;
        timer = setInterval(() => {
          void cmd<number>(CMD.getBackendMicLevel)
            .then((level) => setMicLevel(Math.max(0, Math.min(1, level))))
            .catch(() => undefined);
        }, 200);
      })
      .catch((error) => setMessage(`麦克风实时检测未启动：${String(error)}`));
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
      if (ownsMic) void cmd(CMD.releaseBackendMic).catch(() => undefined);
    };
  }, [open]);

  async function run(item: SetupCheckResult) {
    setRunning(item.id);
    try {
      const next = await cmd<SetupCheckResult>(item.id === "permissions" ? CMD.requestSetupPermissions : CMD.runSetupCheck, item.id === "permissions" ? undefined : { id: item.id });
      setStatus((current) => current ? { ...current, checks: current.checks.map((check) => check.id === next.id ? next : check) } : current);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  function openAction(action?: string | null) {
    if (!action) return;
    setView("settings");
    setSettingsTab(action === "model" ? "model" : action === "keys" ? "keys" : "audio");
    onClose();
  }

  async function testInjection() {
    testInput.current?.focus();
    setRunning("injection");
    try {
      await cmd(CMD.runInjectionSetupCheck, { text: "说吧！注入测试成功" });
      setMessage("如果下方输入框出现测试文字，注入链路正常。");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  async function finish() {
    try {
      await cmd(CMD.completeOnboarding);
      onClose();
    } catch (error) {
      setMessage(String(error));
    }
  }

  return (
    <Modal open={open} onClose={onClose} title="首次使用体检" className="max-w-2xl">
      <div className="flex flex-col gap-5 p-5">
        <p className="text-sm leading-6 text-[var(--color-fg-muted)]">自动确认麦克风、系统权限、识别能力和快捷键。不会上传测试内容，也不会替你选择云服务。</p>
        <div className="flex items-center gap-3" aria-label={`麦克风实时电平 ${Math.round(micLevel * 100)}%`}>
          <span className="w-24 text-xs text-[var(--color-fg-subtle)]">麦克风实时电平</span>
          <div className="h-2 flex-1 overflow-hidden rounded-full bg-[var(--color-surface)]">
            <div className="h-full rounded-full bg-[var(--color-accent)] transition-[width] motion-reduce:transition-none" style={{ width: `${Math.max(2, micLevel * 100)}%` }} />
          </div>
          <span className="w-10 text-right text-xs tabular-nums text-[var(--color-fg-subtle)]">{Math.round(micLevel * 100)}%</span>
        </div>
        <div className="flex flex-col divide-y divide-[var(--color-line)] rounded-[var(--radius-lg)] border border-[var(--color-line)]">
          {status?.checks.map((item) => {
            const ready = item.status === "ready";
            return <div key={item.id} className="flex items-center gap-3 p-4">
              {running === item.id ? <LoaderCircle className="h-5 w-5 animate-spin text-[var(--color-accent)]" aria-hidden /> : ready ? <CheckCircle2 className="h-5 w-5 text-[var(--color-ok)]" aria-hidden /> : <CircleAlert className="h-5 w-5 text-[var(--color-warn)]" aria-hidden />}
              <div className="min-w-0 flex-1"><strong className="text-sm">{item.title}</strong><p className="mt-1 text-xs text-[var(--color-fg-subtle)]">{item.message}</p></div>
              {item.action && <Button size="sm" onClick={() => openAction(item.action)}>去配置</Button>}
              <Button size="sm" onClick={() => void run(item)} disabled={running !== null}>重查</Button>
            </div>;
          })}
        </div>
        <div className="flex flex-col gap-2">
          <label htmlFor="setup-injection-test" className="text-sm font-medium">文本注入自检</label>
          <div className="flex gap-2"><Input ref={testInput} id="setup-injection-test" placeholder="点击右侧按钮后保持此输入框聚焦" /><Button onClick={() => void testInjection()} disabled={running !== null}>测试注入</Button></div>
        </div>
        {message && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
        <div className="flex justify-between border-t border-[var(--color-line)] pt-4"><Button onClick={() => void refresh()}><Stethoscope className="h-4 w-4" aria-hidden />全部重查</Button><Button variant="primary" onClick={() => void finish()}>完成设置</Button></div>
      </div>
    </Modal>
  );
}
