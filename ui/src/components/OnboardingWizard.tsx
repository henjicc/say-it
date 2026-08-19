import { useEffect, useMemo, useState } from "react";
import {
  CheckCircle2,
  ChevronLeft,
  CircleAlert,
  Cloud,
  Download,
  ExternalLink,
  HardDriveDownload,
  KeyRound,
  LoaderCircle,
  ShieldCheck,
} from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { SecretInput } from "@/components/ui/SecretInput";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import {
  modelInfo,
  useModelCatalogRevision,
  type ModelInfo,
} from "@/features/asr/modelRegistry";
import { OFFLINE_MODEL_RELEASE_URL } from "@/lib/constants";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd, type SetupCheckResult, type SetupStatus } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore, type ProviderProfile } from "@/store/useProviderStore";
import { useUiStore } from "@/store/useUiStore";

const STEPS = [
  { title: "权限", description: "允许录音和文字输入" },
  { title: "识别模型", description: "选择云端或本地模型" },
  { title: "离线模型", description: "按需下载并安装" },
] as const;

type CheckState = "checking" | "ready" | "blocked";

function StateIcon({ state }: { state: CheckState }) {
  if (state === "checking") {
    return <LoaderCircle className="h-5 w-5 shrink-0 animate-spin text-[var(--color-accent)]" aria-hidden />;
  }
  if (state === "ready") {
    return <CheckCircle2 className="h-5 w-5 shrink-0 text-[var(--color-ok)]" aria-hidden />;
  }
  return <CircleAlert className="h-5 w-5 shrink-0 text-[var(--color-warn)]" aria-hidden />;
}

function SetupRow({
  state,
  title,
  description,
  actionLabel,
  onAction,
  disabled,
}: {
  state: CheckState;
  title: string;
  description: string;
  actionLabel?: string;
  onAction?: () => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3.5">
      <StateIcon state={state} />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-[var(--color-fg)]">{title}</p>
        <p className="mt-0.5 text-xs leading-5 text-[var(--color-fg-subtle)]">{description}</p>
      </div>
      {onAction && (
        <Button size="sm" className="shrink-0 whitespace-nowrap" onClick={onAction} disabled={disabled}>
          {actionLabel}
        </Button>
      )}
    </div>
  );
}

function isOfflineModel(item: ModelInfo) {
  return item.protocol.startsWith("local-") || item.protocol === "builtin-macos-speech";
}

function providerHasKey(provider?: ProviderProfile) {
  return Boolean(provider?.status?.configured || provider?.status?.hasApiKey);
}

export function OnboardingWizard({ open, onClose }: { open: boolean; onClose: () => void }) {
  useModelCatalogRevision();
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [step, setStep] = useState(0);
  const [running, setRunning] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [microphoneState, setMicrophoneState] = useState<CheckState>("checking");
  const [microphoneMessage, setMicrophoneMessage] = useState("正在请求麦克风访问…");
  const [apiKeyDraft, setApiKeyDraft] = useState("");

  const selectedModel = useDictPrefs((state) => state.prefs.asrModel);
  const micDeviceId = useDictPrefs((state) => state.prefs.micDeviceId);
  const patchDictPrefs = useDictPrefs((state) => state.patch);
  const providers = useProviderStore((state) => state.profiles);
  const loadProviders = useProviderStore((state) => state.load);
  const updateProviderConfig = useProviderStore((state) => state.updateConfig);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);

  const checks = useMemo(
    () => Object.fromEntries((status?.checks ?? []).map((item) => [item.id, item])) as Partial<Record<SetupCheckResult["id"], SetupCheckResult>>,
    [status],
  );
  const selectedModelInfo = modelInfo(selectedModel);
  const selectedProvider = providers.find((provider) => provider.id === selectedModelInfo?.providerId);
  const requiresApiKey = selectedProvider?.authKind === "api-key";
  const hasApiKey = providerHasKey(selectedProvider);
  const installedOfflineModels = DICTATION_ASR_MODEL_OPTIONS
    .map((option) => modelInfo(option.value))
    .filter((item): item is ModelInfo => Boolean(item && isOfflineModel(item)));

  async function refreshStatus() {
    setStatus(await cmd<SetupStatus>(CMD.getSetupStatus));
  }

  async function verifyMicrophone() {
    setMicrophoneState("checking");
    setMicrophoneMessage("正在请求麦克风访问…");
    try {
      const result = await cmd<{ reused?: boolean }>(CMD.startBackendMic, {
        deviceName: micDeviceId || undefined,
      });
      if (!result.reused) await cmd(CMD.releaseBackendMic).catch(() => undefined);
      setMicrophoneState("ready");
      setMicrophoneMessage("麦克风可以正常使用");
    } catch (error) {
      setMicrophoneState("blocked");
      setMicrophoneMessage(`无法使用麦克风：${String(error)}`);
    }
  }

  useEffect(() => {
    if (!open) {
      setApiKeyDraft("");
      return;
    }
    setStep(0);
    setMessage("");
    setApiKeyDraft("");
    void Promise.all([refreshStatus(), loadProviders()]).catch((error) => setMessage(String(error)));
  }, [loadProviders, open]);

  useEffect(() => {
    if (!open || step !== 0) return;
    void verifyMicrophone();
  }, [micDeviceId, open, step]);

  useEffect(() => {
    setApiKeyDraft("");
  }, [selectedModel]);

  async function runPermissionCheck(item: SetupCheckResult) {
    setRunning(item.id);
    setMessage("");
    try {
      const next = await cmd<SetupCheckResult>(CMD.requestSetupPermissions);
      setStatus((current) => current
        ? { ...current, checks: current.checks.map((check) => check.id === next.id ? next : check) }
        : current);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  async function selectModel(model: string) {
    setRunning("model");
    setMessage("");
    try {
      await patchDictPrefs({ asrModel: model });
    } catch (error) {
      setMessage(`保存模型失败：${String(error)}`);
    } finally {
      setRunning(null);
    }
  }

  async function saveApiKey() {
    if (!selectedProvider || !apiKeyDraft.trim()) return;
    setRunning("api-key");
    setMessage("");
    try {
      await updateProviderConfig(selectedProvider.id, { apiKey: apiKeyDraft.trim() });
      setApiKeyDraft("");
      setMessage(`${selectedProvider.displayName} 的 API Key 已保存。`);
    } catch (error) {
      setMessage(`保存 API Key 失败：${String(error)}`);
    } finally {
      setRunning(null);
    }
  }

  function openSettings(tab: "model" | "plugins") {
    setApiKeyDraft("");
    setView("settings");
    setSettingsTab(tab);
    onClose();
  }

  async function openLink(url: string) {
    try {
      await cmd(CMD.openExternalLink, { url });
    } catch (error) {
      setMessage(`打开下载页失败：${String(error)}`);
    }
  }

  async function openApiKeyPage() {
    try {
      await cmd(CMD.openApiKeyPage);
    } catch (error) {
      setMessage(`打开 API Key 页面失败：${String(error)}`);
    }
  }

  async function finish() {
    setRunning("finish");
    setApiKeyDraft("");
    try {
      await cmd(CMD.completeOnboarding);
      onClose();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setRunning(null);
    }
  }

  const permission = checks.permissions;
  const permissionState: CheckState = running === "permissions"
    ? "checking"
    : permission?.status === "ready"
      ? "ready"
      : "blocked";

  return (
    <Modal open={open} onClose={onClose} title="首次使用设置" className="max-w-[680px]">
      <div className="flex min-h-[450px] flex-col">
        <div className="border-b border-[var(--color-line)] px-6 py-4">
          <div className="flex items-center justify-between gap-4">
            <div>
              <p className="text-sm font-semibold text-[var(--color-fg)]">{STEPS[step].title}</p>
              <p className="mt-1 text-xs text-[var(--color-fg-subtle)]">{STEPS[step].description}</p>
            </div>
            <span className="text-xs tabular-nums text-[var(--color-fg-subtle)]">{step + 1} / {STEPS.length}</span>
          </div>
          <div className="mt-3 grid grid-cols-3 gap-2" aria-label={`引导进度：第 ${step + 1} 步，共 ${STEPS.length} 步`}>
            {STEPS.map((item, index) => (
              <div key={item.title} className={`h-1 rounded-[var(--radius-pill)] ${index <= step ? "bg-[var(--color-accent)]" : "bg-[var(--color-surface-strong)]"}`} />
            ))}
          </div>
        </div>

        <div className="flex flex-1 flex-col gap-4 px-6 py-5">
          {step === 0 && <>
            <div>
              <h2 className="text-lg font-semibold text-[var(--color-fg)]">授予必要权限</h2>
              <p className="mt-2 text-sm leading-6 text-[var(--color-fg-muted)]">只申请语音输入必需的权限；窗口 OCR 的屏幕录制权限会在你真正启用该功能时再询问。</p>
            </div>
            <SetupRow
              state={microphoneState}
              title="麦克风"
              description={microphoneMessage}
              actionLabel={microphoneState === "blocked" ? "重新检查" : undefined}
              onAction={microphoneState === "blocked" ? () => void verifyMicrophone() : undefined}
            />
            {isMacOS ? (
              <SetupRow
                state={permissionState}
                title="文字输入"
                description={permission?.message || "需要辅助功能权限，才能把识别文字输入其他软件"}
                actionLabel={permission?.status === "ready" ? undefined : "授予权限"}
                onAction={permission?.status === "ready" || !permission ? undefined : () => void runPermissionCheck(permission)}
                disabled={running !== null}
              />
            ) : (
              <SetupRow
                state="ready"
                title="文字输入"
                description="Windows 基础听写无需额外授权"
              />
            )}
            <p className="flex items-start gap-2 text-xs leading-5 text-[var(--color-fg-subtle)]"><ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />权限只用于录音和把文字写入当前光标位置。</p>
          </>}

          {step === 1 && <>
            <div>
              <h2 className="text-lg font-semibold text-[var(--color-fg)]">选择主识别模型</h2>
              <p className="mt-2 text-sm leading-6 text-[var(--color-fg-muted)]">云端模型效果稳定但需要密钥；本地模型无需密钥，安装后可完全离线使用。</p>
            </div>
            <Field label="语音识别模型" controlId="onboarding-asr-model">
              <Select
                id="onboarding-asr-model"
                value={selectedModel}
                disabled={running !== null}
                onChange={(event) => void selectModel(event.target.value)}
              >
                {DICTATION_ASR_MODEL_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </Select>
            </Field>
            <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3.5">
              {selectedModelInfo && isOfflineModel(selectedModelInfo)
                ? <HardDriveDownload className="h-5 w-5 shrink-0 text-[var(--color-ok)]" aria-hidden />
                : <Cloud className="h-5 w-5 shrink-0 text-[var(--color-accent-light)]" aria-hidden />}
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-[var(--color-fg)]">{selectedProvider?.displayName || selectedModelInfo?.label || "正在读取模型信息…"}</p>
                <p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">
                  {selectedModelInfo && isOfflineModel(selectedModelInfo)
                    ? "本地运行，不需要 API Key"
                    : requiresApiKey
                      ? hasApiKey ? "云端识别，API Key 已配置" : "云端识别，需要配置 API Key"
                      : "当前模型不需要 API Key"}
                </p>
              </div>
            </div>
            {requiresApiKey && !hasApiKey && selectedProvider && (
              <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] p-4">
                <div className="flex items-center gap-2 text-sm font-medium text-[var(--color-fg)]"><KeyRound className="h-4 w-4 text-[var(--color-accent-light)]" aria-hidden />配置 {selectedProvider.displayName} 密钥</div>
                <p className="mt-1.5 text-xs leading-5 text-[var(--color-fg-subtle)]">也可以稍后前往“设置 → 模型 → ASR 供应商”配置。</p>
                <div className="mt-3 flex items-stretch gap-2">
                  <div className="min-w-0 flex-1">
                    <SecretInput
                      aria-label={`${selectedProvider.displayName} API Key`}
                      draftValue={apiKeyDraft}
                      hasStoredValue={false}
                      placeholder="输入 API Key"
                      disabled={running !== null}
                      onDraftChange={setApiKeyDraft}
                    />
                  </div>
                  <Button variant="primary" className="shrink-0" disabled={!apiKeyDraft.trim() || running !== null} onClick={() => void saveApiKey()}>
                    {running === "api-key" ? "保存中…" : "保存"}
                  </Button>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {selectedProvider.kind === "alibabacloud-funasr" && <Button size="sm" onClick={() => void openApiKeyPage()}>获取 API Key<ExternalLink className="h-3.5 w-3.5" aria-hidden /></Button>}
                  <Button size="sm" onClick={() => openSettings("model")}>打开完整设置</Button>
                </div>
              </div>
            )}
          </>}

          {step === 2 && <>
            <div>
              <h2 className="text-lg font-semibold text-[var(--color-fg)]">需要离线使用？</h2>
              <p className="mt-2 text-sm leading-6 text-[var(--color-fg-muted)]">离线模型是独立的 .sayit 模型包，不随安装程序下载。现在可以安装，也可以以后再做。</p>
            </div>
            <div className="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line)]">
              {[
                ["1", "下载模型包", "在官方模型下载页选择实时 Paraformer 或整句 SenseVoice。"],
                ["2", "安装 .sayit", "下载后双击文件，或在“设置 → 插件”中选择安装。"],
                ["3", "选择模型", "安装并启用后，模型会自动出现在语音识别下拉框。"],
              ].map(([number, title, description], index) => (
                <div key={number} className={`flex items-start gap-3 px-4 py-3 ${index ? "border-t border-[var(--color-line)]" : ""}`}>
                  <span className="grid h-6 w-6 shrink-0 place-items-center rounded-[var(--radius-pill)] bg-[var(--accent-soft)] text-xs font-semibold text-[var(--color-accent-light)]">{number}</span>
                  <div><p className="text-sm font-medium text-[var(--color-fg)]">{title}</p><p className="mt-0.5 text-xs leading-5 text-[var(--color-fg-subtle)]">{description}</p></div>
                </div>
              ))}
            </div>
            {installedOfflineModels.length > 0 && (
              <p className="flex items-center gap-2 text-xs text-[var(--color-ok)]"><CheckCircle2 className="h-4 w-4" aria-hidden />已检测到 {installedOfflineModels.length} 个可用离线模型</p>
            )}
            <div className="flex flex-wrap gap-2">
              <Button variant="primary" onClick={() => void openLink(OFFLINE_MODEL_RELEASE_URL)}><Download className="h-4 w-4" aria-hidden />打开模型下载页</Button>
              <Button onClick={() => openSettings("plugins")}>打开插件管理</Button>
            </div>
          </>}

          {message && <p role="status" className="mt-auto text-xs leading-5 text-[var(--color-fg-subtle)]">{message}</p>}
        </div>

        <div className="flex items-center justify-between border-t border-[var(--color-line)] px-6 py-4">
          <Button onClick={() => setStep((current) => Math.max(0, current - 1))} disabled={step === 0 || running !== null}>
            <ChevronLeft className="h-4 w-4" aria-hidden />上一步
          </Button>
          {step < STEPS.length - 1
            ? <Button variant="primary" onClick={() => setStep((current) => Math.min(STEPS.length - 1, current + 1))} disabled={running !== null}>下一步</Button>
            : <Button variant="primary" onClick={() => void finish()} disabled={running !== null}>{running === "finish" ? "正在保存…" : "完成设置"}</Button>}
        </div>
      </div>
    </Modal>
  );
}
