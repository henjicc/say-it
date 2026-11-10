import { useEffect, useState } from "react";
import { AsrStep } from "@/components/onboarding/AsrStep";
import { PermissionStep } from "@/components/onboarding/PermissionStep";
import { SmartStep } from "@/components/onboarding/SmartStep";
import { TriggerStep } from "@/components/onboarding/TriggerStep";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { cn } from "@/lib/cn";
import type { AsrChoiceKey } from "@/features/onboarding/setup";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";

const STEPS = ["权限", "识别模型", "智能整理", "唤起方式"] as const;

/**
 * 使用引导。首次启动（`required`）时没有识别模型就用不了，只能走完，不提供跳过或关闭；
 * 从设置页重新运行时可以随时关闭。
 */
export function OnboardingWizard({ open, required, onClose }: { open: boolean; required: boolean; onClose: () => void }) {
  const [step, setStep] = useState(0);
  const [asrChoice, setAsrChoice] = useState<AsrChoiceKey>("bailian");
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState("");
  const loadProviders = useProviderStore((state) => state.load);

  useEffect(() => {
    if (!open) return;
    setStep(0);
    setError("");
    // 读取失败由供应商 store 记在 statusText，识别模型一步里显示并可重试。
    void loadProviders();
  }, [loadProviders, open]);

  const back = () => setStep((current) => Math.max(0, current - 1));
  const next = () => setStep((current) => Math.min(STEPS.length - 1, current + 1));

  const finish = async () => {
    setFinishing(true);
    setError("");
    try {
      // 必须落盘后再关闭，避免重启后又弹出引导。
      await cmd(CMD.completeOnboarding);
      onClose();
    } catch (reason) {
      setError(`保存引导状态失败：${String(reason)}`);
    } finally {
      setFinishing(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={required ? undefined : onClose}
      showHeader={false}
      ariaLabel="首次使用设置"
      className="h-[var(--onboarding-dialog-h)] max-w-[var(--onboarding-dialog-w)]"
      bodyClassName="flex flex-col overflow-hidden"
    >
      <div className="shrink-0 border-b border-[var(--color-line)] px-6 pt-5">
        <div className="flex items-center justify-between gap-4">
          <h2 className="text-lg font-semibold text-[var(--color-fg)]">欢迎使用说吧！</h2>
          {!required && <Button variant="text" size="sm" onClick={onClose}>关闭</Button>}
        </div>
        <ol className="mt-3 flex gap-5" aria-label="设置步骤">
          {STEPS.map((title, index) => (
            <li
              key={title}
              aria-current={index === step ? "step" : undefined}
              className={cn(
                "border-b-2 pb-2.5 text-xs",
                index === step
                  ? "border-[var(--color-accent)] text-[var(--color-fg)]"
                  : cn("border-transparent", index < step ? "text-[var(--color-fg-muted)]" : "text-[var(--color-fg-subtle)]"),
              )}
            >
              {title}
            </li>
          ))}
        </ol>
      </div>
      {step === 0 && <PermissionStep onNext={next} />}
      {step === 1 && (
        <AsrStep
          choice={asrChoice}
          onChoice={setAsrChoice}
          onBack={back}
          onNext={next}
        />
      )}
      {step === 2 && <SmartStep asrChoice={asrChoice} onBack={back} onNext={next} />}
      {step === 3 && <TriggerStep onBack={back} onFinish={() => void finish()} finishing={finishing} error={error} />}
    </Modal>
  );
}
