import { useEffect, useState } from "react";
import { DataDirModal } from "@/components/onboarding/DataDirModal";
import { ModeStep } from "@/components/onboarding/ModeStep";
import { PermissionStep } from "@/components/onboarding/PermissionStep";
import { PlanStep } from "@/components/onboarding/PlanStep";
import { TriggerStep } from "@/components/onboarding/TriggerStep";
import "@/components/onboarding/Onboarding.css";
import { AppMark } from "@/components/shell/AppMark";
import { Button } from "@/components/ui/Button";
import { useHardware, type HardwareSummary } from "@/features/models/localModelNeeds";
import { useLocalModelProgress } from "@/features/models/localModels";
import { providerReady } from "@/features/models/providerModels";
import {
  applyDeepSeekMix,
  applyLocalPlan,
  applyOnlineBundle,
  bundleAlreadyConfigured,
  deepseekAlreadyConfigured,
  LOCAL_LITE_PACKS,
  localPlanPacks,
  localPolishDisallowed,
  localPolishFits,
  ONLINE_BUNDLES,
  type LocalAsrTier,
  type LocalPlanKey,
  type OnlineBundleId,
  type UsageMode,
} from "@/features/onboarding/setup";
import { setQueryData, useCachedQuery } from "@/hooks/useCachedQuery";
import { cn } from "@/lib/cn";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd, type DataRootStatus } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";
import { useUiStore } from "@/store/useUiStore";
import { dataRootQuery } from "@/views/SettingsStartupPanel";

const STEPS = ["准备", "模式", "方案", "快捷键"] as const;

function hardwarePillText(hardware: HardwareSummary | undefined): string {
  if (!hardware || (!hardware.gpuMemoryMb && !hardware.memoryMb)) return "";
  if (hardware.unifiedMemory) {
    return hardware.gpuName ? `macOS · ${hardware.gpuName}` : "macOS · Apple 芯片";
  }
  const os = isMacOS ? "macOS" : "Windows";
  if (hardware.gpuMemoryMb >= 1024) {
    return `${os} · 独显 ${Math.round(hardware.gpuMemoryMb / 1024)} GB`;
  }
  return `${os} · 无独显`;
}

/**
 * 覆盖主窗口的一体式 4 步用户引导。
 * 首次启动（`required`）时走完才可进入主界面；从设置页重新运行时可随时按 Esc 或点「关闭」退出。
 */
export function OnboardingWizard({
  open,
  required,
  onClose,
}: {
  open: boolean;
  required: boolean;
  onClose: () => void;
}) {
  const [step, setStep] = useState(0);
  const hardware = useHardware();
  const layoutMode = useUiStore((state) => state.layoutMode);
  const { data: dataRoot } = useCachedQuery(dataRootQuery);
  const [dirModalOpen, setDirModalOpen] = useState(false);

  const [pickedMode, setPickedMode] = useState<UsageMode>();
  const mode: UsageMode = pickedMode ?? (localPolishFits(hardware) ? "local" : "online");

  const [pickedLocalPlan, setPickedLocalPlan] = useState<LocalPlanKey>();
  const localPlan: LocalPlanKey = localPolishDisallowed(hardware)
    ? "lite"
    : (pickedLocalPlan ?? (localPolishFits(hardware) ? "full" : "lite"));
  const [asrTier, setAsrTier] = useState<LocalAsrTier>("0.6b");

  const [onlineId, setOnlineId] = useState<OnlineBundleId>("bailian");
  const [mixActive, setMixActive] = useState(false);
  const [onlineKeys, setOnlineKeys] = useState<Record<string, Record<string, string>>>({});
  const [deepseekKey, setDeepseekKey] = useState("");

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const profiles = useProviderStore((state) => state.profiles);
  const loadProviders = useProviderStore((state) => state.load);

  const activeLocalPackIds = (
    mode === "local"
      ? localPlanPacks(localPlan, asrTier)
      : mixActive
        ? LOCAL_LITE_PACKS
        : []
  ).map((item) => item.id);
  const packProgress = useLocalModelProgress(activeLocalPackIds);

  useEffect(() => {
    if (!open) return;
    setStep(0);
    setError("");
    setSaving(false);
    setDirModalOpen(false);
    void loadProviders();
  }, [loadProviders, open]);

  useEffect(() => {
    if (!open || required) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !dirModalOpen) onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dirModalOpen, onClose, open, required]);

  if (!open) return null;

  const currentBundle = ONLINE_BUNDLES.find((item) => item.id === onlineId) ?? ONLINE_BUNDLES[0];
  const currentProfile = profiles.find((item) => item.id === currentBundle.providerId);

  const onlineKeysReady = (() => {
    if (mixActive) {
      return (deepseekAlreadyConfigured(profiles) || Boolean(deepseekKey.trim())) && packProgress.ready;
    }
    if (bundleAlreadyConfigured(currentBundle, profiles)) return true;
    return currentBundle.keys.every((spec) => {
      const existing = Boolean(currentProfile && providerReady(currentProfile, spec.capability));
      const draft = Boolean((onlineKeys[currentBundle.id]?.[spec.field] ?? "").trim());
      return existing || draft;
    });
  })();

  let canNext = !saving;
  let footerHint = "";
  let nextLabel = "下一步 →";

  if (step === 2) {
    if (mode === "local") {
      canNext = !saving && packProgress.ready;
      footerHint = packProgress.downloading ? "下载完成后即可继续" : "先点击「开始下载」准备模型";
    } else if (mixActive) {
      const keyOk = deepseekAlreadyConfigured(profiles) || Boolean(deepseekKey.trim());
      canNext = !saving && keyOk && packProgress.ready;
      footerHint = !keyOk
        ? "请填入 DeepSeek API Key"
        : packProgress.downloading
          ? "等待本地模型下载完成"
          : "请先下载本地免费识别模型";
    } else {
      canNext = !saving && onlineKeysReady;
      footerHint = "填好密钥即可继续";
    }
  } else if (step === 3) {
    nextLabel = saving ? "正在保存…" : "开始使用";
  }

  const handleNext = async () => {
    setError("");
    if (step === 2) {
      setSaving(true);
      try {
        if (mode === "local") {
          await applyLocalPlan(localPlan, asrTier);
        } else if (mixActive) {
          await applyDeepSeekMix(deepseekKey);
        } else {
          await applyOnlineBundle(currentBundle, onlineKeys[currentBundle.id] ?? {});
        }
        setStep(3);
      } catch (reason) {
        setError(`保存方案配置失败：${String(reason)}`);
      } finally {
        setSaving(false);
      }
      return;
    }
    if (step === 3) {
      setSaving(true);
      try {
        // 必须落盘后再关闭，避免重启后又弹出引导。
        await cmd(CMD.completeOnboarding);
        onClose();
      } catch (reason) {
        setError(`保存引导状态失败：${String(reason)}`);
      } finally {
        setSaving(false);
      }
      return;
    }
    setStep((current) => Math.min(STEPS.length - 1, current + 1));
  };

  const handleBack = () => {
    setError("");
    setStep((current) => Math.max(0, current - 1));
  };

  const envBadge = hardwarePillText(hardware);
  const macPro = isMacOS && layoutMode === "pro";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="首次使用设置"
      className={cn(
        "ob-shell fixed inset-x-0 bottom-0 z-[var(--z-modal)] flex flex-col overflow-hidden",
        macPro ? "top-0" : "top-[var(--titlebar-h)]",
      )}
    >
      {/* 极简浮动顶栏 */}
      <header
        data-tauri-drag-region={macPro ? "" : undefined}
        className={cn(
          "flex flex-none items-center justify-between gap-4 px-6 pt-5 pb-2 select-none",
          macPro && "pl-[86px]",
        )}
      >
        <div className="flex items-center gap-2.5">
          <AppMark />
          <span className="text-[14px] font-semibold tracking-tight text-[var(--color-fg)]">说吧！</span>
        </div>

        <nav aria-label="设置步骤" className="no-drag flex items-center gap-2">
          {STEPS.map((label, index) => {
            const active = index === step;
            const passed = index < step;
            return (
              <button
                key={label}
                type="button"
                title={label}
                aria-label={`第 ${index + 1} 步：${label}`}
                aria-current={active ? "step" : undefined}
                disabled={index > step}
                onClick={() => {
                  if (index < step) {
                    setError("");
                    setStep(index);
                  }
                }}
                className={cn(
                  "flex items-center py-1 transition-all",
                  index <= step ? "cursor-pointer" : "cursor-default",
                )}
              >
                <span
                  className="h-1.5 rounded-full transition-all duration-200"
                  style={{
                    width: active ? "28px" : "8px",
                    backgroundColor: active || passed ? "var(--color-accent)" : "var(--color-line-strong)",
                    opacity: passed ? 0.45 : 1,
                  }}
                />
              </button>
            );
          })}
        </nav>

        <div className="no-drag flex items-center gap-2">
          {envBadge && (
            <span className="hidden items-center gap-1.5 rounded-full border border-[var(--color-line)] bg-[var(--color-surface)] px-2.5 py-1 text-xs text-[var(--color-fg-muted)] sm:inline-flex">
              <span
                className="h-1.5 w-1.5 rounded-full"
                style={{
                  backgroundColor: localPolishFits(hardware) ? "var(--color-ok)" : "var(--color-warn)",
                }}
              />
              <span>{envBadge}</span>
            </span>
          )}
          {!required && (
            <Button variant="text" size="sm" onClick={onClose}>
              关闭
            </Button>
          )}
        </div>
      </header>

      {/* 主内容区（宽窗/窄窗自适应，居中呼吸感） */}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-6 py-4">
        <div key={`${step}-${mode}`} className="ob-step-enter mx-auto my-auto w-full max-w-[740px]">
          {step === 0 && (
            <PermissionStep
              dataRoot={dataRoot}
              onOpenDirPicker={() => setDirModalOpen(true)}
            />
          )}
          {step === 1 && (
            <ModeStep
              mode={mode}
              onMode={(nextMode) => {
                setError("");
                setPickedMode(nextMode);
              }}
              hardware={hardware}
            />
          )}
          {step === 2 && (
            <PlanStep
              mode={mode}
              hardware={hardware}
              dataRoot={dataRoot}
              localPlan={localPlan}
              asrTier={asrTier}
              onlineId={onlineId}
              mixActive={mixActive}
              onlineKeys={onlineKeys}
              deepseekKey={deepseekKey}
              error={error}
              onSwitchToOnline={() => {
                setError("");
                setPickedMode("online");
              }}
              onOpenDirPicker={() => setDirModalOpen(true)}
              onLocalPlan={(plan) => {
                setError("");
                setPickedLocalPlan(plan);
              }}
              onAsrTier={setAsrTier}
              onOnlineId={(id) => {
                setError("");
                setOnlineId(id);
              }}
              onMixActive={(active) => {
                setError("");
                setMixActive(active);
              }}
              onOnlineKeyChange={(bundleId, field, value) => {
                setError("");
                setOnlineKeys((prev) => ({
                  ...prev,
                  [bundleId]: { ...(prev[bundleId] ?? {}), [field]: value },
                }));
              }}
              onDeepseekKeyChange={(value) => {
                setError("");
                setDeepseekKey(value);
              }}
            />
          )}
          {step === 3 && <TriggerStep error={error} />}
        </div>

        {/* 浮动无边框底栏 */}
        <footer className="mx-auto flex w-full max-w-[740px] flex-none items-center justify-between gap-4 pt-6 pb-2">
          <div>
            {step > 0 ? (
              <Button variant="text" onClick={handleBack} disabled={saving} className="rounded-full px-4">
                ← 上一步
              </Button>
            ) : (
              <span />
            )}
          </div>
          <div className="flex items-center gap-3">
            {!canNext && footerHint && (
              <span className="text-xs text-[var(--color-fg-muted)]">{footerHint}</span>
            )}
            <Button
              variant="primary"
              disabled={!canNext}
              onClick={() => void handleNext()}
              className="h-10 rounded-full px-6 text-[13.5px]"
            >
              {nextLabel}
            </Button>
          </div>
        </footer>
      </div>

      <DataDirModal
        open={dirModalOpen}
        status={dataRoot}
        onClose={() => setDirModalOpen(false)}
        onApplied={(next: DataRootStatus) => {
          setQueryData(dataRootQuery, next);
        }}
      />
    </div>
  );
}
