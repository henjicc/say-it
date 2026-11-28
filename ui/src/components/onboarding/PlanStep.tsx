import { useState } from "react";
import { Input } from "@/components/ui/Input";
import type { HardwareSummary } from "@/features/models/localModelNeeds";
import { useLocalModelProgress } from "@/features/models/localModels";
import { providerReady } from "@/features/models/providerModels";
import {
  apiKeyUrlForBundle,
  bundleAlreadyConfigured,
  deepseekAlreadyConfigured,
  deepseekPreset,
  LOCAL_FULL_ASR_TIERS,
  LOCAL_LITE_PACKS,
  localPlanPacks,
  localPolishDisallowed,
  localPolishFits,
  localPolishVramHint,
  ONLINE_BUNDLES,
  type LocalAsrTier,
  type LocalPlanKey,
  type OnlineBundleId,
  type UsageMode,
} from "@/features/onboarding/setup";
import { ApiKeyLink } from "@/features/settings/apiKeyLinks";
import { cn } from "@/lib/cn";
import type { DataRootStatus } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";
import { PackDownloadPanel } from "./PackDownloadPanel";

/**
 * 第 3 步 · 一键配好方案（离线模式套餐 / 在线一站式服务商 + 折叠 DeepSeek 混搭）。
 */
export function PlanStep({
  mode,
  hardware,
  dataRoot,
  localPlan,
  asrTier,
  onlineId,
  mixActive,
  onlineKeys,
  deepseekKey,
  error,
  onSwitchToOnline,
  onOpenDirPicker,
  onLocalPlan,
  onAsrTier,
  onOnlineId,
  onMixActive,
  onOnlineKeyChange,
  onDeepseekKeyChange,
}: {
  mode: UsageMode;
  hardware: HardwareSummary | undefined;
  dataRoot: DataRootStatus | undefined;
  localPlan: LocalPlanKey;
  asrTier: LocalAsrTier;
  onlineId: OnlineBundleId;
  mixActive: boolean;
  onlineKeys: Record<string, Record<string, string>>;
  deepseekKey: string;
  error: string;
  onSwitchToOnline: () => void;
  onOpenDirPicker: () => void;
  onLocalPlan: (plan: LocalPlanKey) => void;
  onAsrTier: (tier: LocalAsrTier) => void;
  onOnlineId: (id: OnlineBundleId) => void;
  onMixActive: (active: boolean) => void;
  onOnlineKeyChange: (bundleId: OnlineBundleId, field: string, value: string) => void;
  onDeepseekKeyChange: (value: string) => void;
}) {
  if (mode === "local") {
    return (
      <LocalPlanSection
        hardware={hardware}
        dataRoot={dataRoot}
        localPlan={localPlan}
        asrTier={asrTier}
        error={error}
        onSwitchToOnline={onSwitchToOnline}
        onOpenDirPicker={onOpenDirPicker}
        onLocalPlan={onLocalPlan}
        onAsrTier={onAsrTier}
      />
    );
  }
  return (
    <OnlinePlanSection
      onlineId={onlineId}
      mixActive={mixActive}
      onlineKeys={onlineKeys}
      deepseekKey={deepseekKey}
      error={error}
      onOnlineId={onOnlineId}
      onMixActive={onMixActive}
      onOnlineKeyChange={onOnlineKeyChange}
      onDeepseekKeyChange={onDeepseekKeyChange}
    />
  );
}

function LocalPlanSection({
  hardware,
  dataRoot,
  localPlan,
  asrTier,
  error,
  onSwitchToOnline,
  onOpenDirPicker,
  onLocalPlan,
  onAsrTier,
}: {
  hardware: HardwareSummary | undefined;
  dataRoot: DataRootStatus | undefined;
  localPlan: LocalPlanKey;
  asrTier: LocalAsrTier;
  error: string;
  onSwitchToOnline: () => void;
  onOpenDirPicker: () => void;
  onLocalPlan: (plan: LocalPlanKey) => void;
  onAsrTier: (tier: LocalAsrTier) => void;
}) {
  const gpuFits = localPolishFits(hardware);
  const fullDisabled = localPolishDisallowed(hardware);
  const items = localPlanPacks(localPlan, asrTier);
  const { downloading } = useLocalModelProgress(items.map((item) => item.id));
  const activeDir = dataRoot?.configuredRoot ?? "读取中…";

  const plans = [
    {
      key: "lite" as const,
      title: "轻量极速",
      badge: fullDisabled ? "适合你的电脑" : "极省资源",
      disabled: false,
      size: "约 650 MB",
      points: [
        "纯 CPU 流畅运行，不挑电脑",
        "Fun-ASR-Nano + 中英自动标点 + 本地去语气词",
      ],
    },
    {
      key: "full" as const,
      title: "全能本地",
      badge: gpuFits ? "推荐" : fullDisabled ? "需要独立显卡" : "高配推荐",
      disabled: fullDisabled,
      size: asrTier === "1.7b" ? "约 5.3 GB" : "约 4.1 GB",
      points: [
        "Qwen3-ASR 听写 + 本地 AI 智能润色",
        "理顺语序、长段分点、保留原本语气",
      ],
    },
  ];

  return (
    <div className="space-y-5" aria-label="选择离线方案">
      <div className="space-y-1.5 text-center">
        <h1 className="text-[24px] font-semibold tracking-tight text-[var(--color-fg)]">选择离线方案</h1>
        <p className="text-[13px] text-[var(--color-fg-muted)]">
          将自动下载到{" "}
          <span className="inline-block max-w-[340px] truncate rounded-md bg-[var(--color-surface-hover)] px-1.5 py-0.5 align-bottom font-mono text-xs text-[var(--color-fg)]" title={activeDir}>
            {activeDir}
          </span>
          <button
            type="button"
            disabled={downloading || !dataRoot || dataRoot.restartRequired}
            onClick={onOpenDirPicker}
            className="ml-1.5 text-xs font-medium text-[var(--color-accent-text)] hover:underline disabled:cursor-not-allowed disabled:opacity-45"
          >
            更改
          </button>
        </p>
      </div>

      {fullDisabled && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-lg)] bg-[color-mix(in_srgb,var(--color-warn)_12%,transparent)] px-4 py-3 text-xs text-[var(--color-warn)]">
          <span>{localPolishVramHint(hardware)}</span>
          <button
            type="button"
            onClick={onSwitchToOnline}
            className="shrink-0 font-medium underline"
          >
            改选在线
          </button>
        </div>
      )}

      <div role="radiogroup" aria-label="离线方案" className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {plans.map((plan) => {
          const selected = localPlan === plan.key;
          return (
            <div
              key={plan.key}
              role="radio"
              tabIndex={plan.disabled ? -1 : 0}
              aria-checked={selected}
              aria-disabled={plan.disabled}
              onClick={() => {
                if (!plan.disabled) onLocalPlan(plan.key);
              }}
              onKeyDown={(event) => {
                if (plan.disabled) return;
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onLocalPlan(plan.key);
                }
              }}
              className={cn(
                "flex flex-col justify-between gap-4 rounded-[var(--radius-xl)] bg-[var(--color-surface)] p-5 transition-all",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                plan.disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer",
                selected && !plan.disabled
                  ? "border-[1.5px] border-[var(--color-accent)] shadow-[0_0_0_4px_var(--accent-soft),var(--shadow-raise)]"
                  : "border-[1.5px] border-[var(--color-line)] shadow-[var(--shadow-raise)] hover:border-[var(--color-line-strong)]",
              )}
            >
              <div className="space-y-3.5">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span className="text-[16px] font-semibold text-[var(--color-fg)]">{plan.title}</span>
                    <span
                      className={cn(
                        "rounded-full px-2 py-0.5 text-[11px] font-medium",
                        selected && !plan.disabled
                          ? "bg-[var(--accent-soft)] text-[var(--color-accent-text)]"
                          : "bg-[var(--color-surface-hover)] text-[var(--color-fg-muted)]",
                      )}
                    >
                      {plan.badge}
                    </span>
                  </div>
                  <span className="font-mono text-xs text-[var(--color-fg-muted)]">{plan.size}</span>
                </div>
                <ul className="space-y-2 text-[13px] text-[var(--color-fg-muted)]">
                  {plan.points.map((point) => (
                    <li key={point} className="flex items-start gap-2">
                      <span className="text-[var(--color-accent)]" aria-hidden>•</span>
                      <span>{point}</span>
                    </li>
                  ))}
                </ul>
              </div>

              {plan.key === "full" && selected && !plan.disabled && (
                <div
                  className="flex items-center justify-between border-t border-[var(--color-line)] pt-3 text-xs"
                  onClick={(event) => event.stopPropagation()}
                >
                  <span className="text-[var(--color-fg-muted)]">听写底座</span>
                  <div className="inline-flex rounded-[var(--radius-md)] bg-[var(--color-surface-hover)] p-0.5">
                    {(["0.6b", "1.7b"] as const).map((tier) => {
                      const active = asrTier === tier;
                      return (
                        <button
                          key={tier}
                          type="button"
                          onClick={() => onAsrTier(tier)}
                          className={cn(
                            "cursor-pointer rounded-[6px] px-2.5 py-1 transition-colors",
                            active
                              ? "bg-[var(--color-accent-control)] font-medium text-[var(--color-accent-control-contrast)]"
                              : "text-[var(--color-fg-muted)] hover:text-[var(--color-fg)]",
                          )}
                        >
                          {LOCAL_FULL_ASR_TIERS[tier].label}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      <PackDownloadPanel items={items} />

      {error && (
        <p role="alert" className="text-center text-xs text-[var(--color-err)]">
          {error}
        </p>
      )}
    </div>
  );
}

function OnlinePlanSection({
  onlineId,
  mixActive,
  onlineKeys,
  deepseekKey,
  error,
  onOnlineId,
  onMixActive,
  onOnlineKeyChange,
  onDeepseekKeyChange,
}: {
  onlineId: OnlineBundleId;
  mixActive: boolean;
  onlineKeys: Record<string, Record<string, string>>;
  deepseekKey: string;
  error: string;
  onOnlineId: (id: OnlineBundleId) => void;
  onMixActive: (active: boolean) => void;
  onOnlineKeyChange: (bundleId: OnlineBundleId, field: string, value: string) => void;
  onDeepseekKeyChange: (value: string) => void;
}) {
  const profiles = useProviderStore((state) => state.profiles);
  const [linkError, setLinkError] = useState("");
  const [editingConfigured, setEditingConfigured] = useState<Record<string, boolean>>({});

  const currentBundle = ONLINE_BUNDLES.find((item) => item.id === onlineId) ?? ONLINE_BUNDLES[0];
  const currentProfile = profiles.find((item) => item.id === currentBundle.providerId);
  const bundleConfigured = bundleAlreadyConfigured(currentBundle, profiles);
  const dsConfigured = deepseekAlreadyConfigured(profiles);
  const dsPreset = deepseekPreset();

  const showBundleInputs = !bundleConfigured || editingConfigured[currentBundle.id];
  const showDeepseekInput = !dsConfigured || editingConfigured.deepseek;

  return (
    <div className="space-y-4" aria-label="选择在线服务商">
      <div className="space-y-1.5 text-center">
        <h1 className="text-[24px] font-semibold tracking-tight text-[var(--color-fg)]">选择在线服务商</h1>
        <p className="text-[14px] text-[var(--color-fg-muted)]">
          优先推荐同时提供「语音识别 + 大模型润色」的一站式平台。
        </p>
      </div>

      <div role="radiogroup" aria-label="在线服务商" className="grid grid-cols-1 gap-3.5 md:grid-cols-2">
        {ONLINE_BUNDLES.map((bundle) => {
          const selected = !mixActive && onlineId === bundle.id;
          const configured = bundleAlreadyConfigured(bundle, profiles);
          return (
            <button
              key={bundle.id}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => {
                onMixActive(false);
                onOnlineId(bundle.id);
              }}
              className={cn(
                "cursor-pointer space-y-2 rounded-[var(--radius-xl)] bg-[var(--color-surface)] p-4 text-left transition-all",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                selected
                  ? "border-[1.5px] border-[var(--color-accent)] shadow-[0_0_0_4px_var(--accent-soft),var(--shadow-raise)]"
                  : "border-[1.5px] border-[var(--color-line)] shadow-[var(--shadow-raise)] hover:border-[var(--color-line-strong)]",
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-[15px] font-semibold text-[var(--color-fg)]">{bundle.name}</span>
                <span
                  className={cn(
                    "rounded-full px-2 py-0.5 text-[11px] font-medium",
                    configured
                      ? "bg-[color-mix(in_srgb,var(--color-ok)_12%,transparent)] text-[var(--color-ok)]"
                      : bundle.accent
                        ? "bg-[var(--accent-soft)] text-[var(--color-accent-text)]"
                        : "bg-[var(--color-surface-hover)] text-[var(--color-fg-muted)]",
                  )}
                >
                  {configured ? "已配置" : bundle.tag}
                </span>
              </div>
              <div className="text-[13px] text-[var(--color-fg)]">{bundle.line}</div>
              <div className="text-xs text-[var(--color-fg-muted)]">{bundle.note}</div>
            </button>
          );
        })}
      </div>

      {/* 密钥输入与配置区 */}
      <div className="space-y-3 rounded-[var(--radius-xl)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-raise)]">
        {mixActive ? (
          <>
            <div className="flex items-center justify-between gap-3 text-xs">
              <span className="text-[13px] font-medium text-[var(--color-fg)]">
                DeepSeek 官方 API Key（搭配本地免费识别）
              </span>
              <div className="flex items-center gap-3">
                {dsConfigured && !showDeepseekInput && (
                  <button
                    type="button"
                    onClick={() => setEditingConfigured((prev) => ({ ...prev, deepseek: true }))}
                    className="text-[var(--color-fg-muted)] hover:text-[var(--color-fg)]"
                  >
                    更换密钥
                  </button>
                )}
                {dsPreset.apiKeyUrl && (
                  <ApiKeyLink url={dsPreset.apiKeyUrl} label="获取密钥" onError={setLinkError} />
                )}
              </div>
            </div>
            {showDeepseekInput ? (
              <Input
                type="password"
                aria-label="DeepSeek API Key"
                value={deepseekKey}
                onChange={(event) => onDeepseekKeyChange(event.target.value)}
                placeholder="DeepSeek API Key（sk-...）"
                autoComplete="off"
                spellCheck={false}
              />
            ) : (
              <p className="text-xs font-medium text-[var(--color-ok)]">✓ 已配置 DeepSeek 密钥</p>
            )}
          </>
        ) : (
          <>
            <div className="flex items-center justify-between gap-3 text-xs">
              <span className="text-[13px] font-medium text-[var(--color-fg)]">
                填写 {currentBundle.name} 密钥
              </span>
              <div className="flex items-center gap-3">
                {bundleConfigured && !showBundleInputs && (
                  <button
                    type="button"
                    onClick={() => setEditingConfigured((prev) => ({ ...prev, [currentBundle.id]: true }))}
                    className="text-[var(--color-fg-muted)] hover:text-[var(--color-fg)]"
                  >
                    更换密钥
                  </button>
                )}
                {currentBundle.keys.length === 1 && (() => {
                  const url = apiKeyUrlForBundle(currentBundle, currentBundle.keys[0], currentProfile);
                  return url ? <ApiKeyLink url={url} label="获取密钥" onError={setLinkError} /> : null;
                })()}
              </div>
            </div>

            {showBundleInputs ? (
              <div className={cn("grid gap-3", currentBundle.keys.length > 1 ? "grid-cols-1 md:grid-cols-2" : "grid-cols-1")}>
                {currentBundle.keys.map((spec) => {
                  const keyReady = Boolean(currentProfile && providerReady(currentProfile, spec.capability));
                  const url = currentBundle.keys.length > 1
                    ? apiKeyUrlForBundle(currentBundle, spec, currentProfile)
                    : undefined;
                  return (
                    <div key={spec.field} className="space-y-1.5">
                      {currentBundle.keys.length > 1 && (
                        <div className="flex items-center justify-between text-xs text-[var(--color-fg-muted)]">
                          <label htmlFor={`ob-key-${currentBundle.id}-${spec.field}`}>
                            {spec.label}
                            {keyReady && <span className="ml-1.5 text-[var(--color-ok)]">（已填）</span>}
                          </label>
                          {url && <ApiKeyLink url={url} label="获取" onError={setLinkError} />}
                        </div>
                      )}
                      <Input
                        id={`ob-key-${currentBundle.id}-${spec.field}`}
                        type="password"
                        aria-label={`${currentBundle.name} ${spec.label}`}
                        value={onlineKeys[currentBundle.id]?.[spec.field] ?? ""}
                        onChange={(event) => onOnlineKeyChange(currentBundle.id, spec.field, event.target.value)}
                        placeholder={
                          keyReady
                            ? `${spec.label}（留空保持当前密钥）`
                            : `${spec.label}（${spec.placeholder}）`
                        }
                        autoComplete="off"
                        spellCheck={false}
                      />
                    </div>
                  );
                })}
              </div>
            ) : (
              <p className="text-xs font-medium text-[var(--color-ok)]">
                ✓ 已配置 {currentBundle.name} 密钥，下一步将自动配齐识别与润色模型
              </p>
            )}
          </>
        )}
      </div>

      {mixActive && (
        <PackDownloadPanel
          items={LOCAL_LITE_PACKS}
          titleIdle="下载本地免费识别模型"
          titleDone="本地免费识别模型已就绪"
        />
      )}

      {(linkError || error) && (
        <p role="alert" className="text-center text-xs text-[var(--color-err)]">
          {error || linkError}
        </p>
      )}

      <div className="text-center">
        <button
          type="button"
          onClick={() => onMixActive(!mixActive)}
          className="cursor-pointer text-xs text-[var(--color-fg-muted)] hover:text-[var(--color-fg)] hover:underline"
        >
          {mixActive ? "← 返回选择一站式服务商" : "只有 DeepSeek 官方 Key？搭配本地免费识别"}
        </button>
      </div>
    </div>
  );
}
