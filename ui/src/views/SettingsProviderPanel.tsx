import { useEffect, useRef, useState } from "react";
import { Collapse } from "@/components/ui/Collapse";
import { Button } from "@/components/ui/Button";
import { Field, CheckField } from "@/components/ui/Field";
import { Input } from "@/components/ui/Input";
import { SecretInput } from "@/components/ui/SecretInput";
import { Slider } from "@/components/ui/Slider";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { ApiKeyLink, ASR_API_KEY_URLS } from "@/features/settings/apiKeyLinks";
import { CMD, cmd } from "@/lib/tauri";
import {
  useProviderStore,
  type ProviderCapability,
  type ProviderProfile,
} from "@/store/useProviderStore";

const NESTED_COLLAPSE_CLASS = "bg-[var(--color-bg)]";
const NESTED_HEADER_CLASS = "px-3 py-2.5";
const NESTED_BODY_CLASS = "px-3 py-3";

const PLUGIN_ACTION_LABELS: Record<string, string> = {
  openLogin: "打开登录窗口",
  syncSession: "获取并保护登录会话",
  clearSession: "清除登录会话",
  diagnose: "运行诊断",
};

type ProviderSectionCapability = Extract<ProviderCapability, "asr" | "ocr" | "translation">;

/** 分区顺序即多能力供应商的“主分区”优先级：完整配置面板只出现在第一个命中的分区。 */
const SECTION_CAPABILITIES: readonly ProviderSectionCapability[] = ["asr", "ocr", "translation"];
const SECTION_TITLES: Record<ProviderSectionCapability, string> = {
  asr: "ASR 供应商",
  ocr: "OCR 供应商",
  translation: "翻译供应商",
};

function primaryCapabilityOf(provider: ProviderProfile): ProviderSectionCapability | undefined {
  return SECTION_CAPABILITIES.find((capability) => provider.capabilities.includes(capability));
}

function providerConfigurationStatus(provider: ProviderProfile) {
  if (provider.authKind === "none") return "无需配置";
  return provider.status?.configured || provider.status?.hasApiKey ? "已配置" : "需要填写";
}

function hasProviderConfiguration(provider: ProviderProfile) {
  return Boolean(
    provider.configFields?.length ||
    provider.actions?.length ||
    provider.capabilities.includes("customization"),
  );
}

function ProviderConfigEditor({ provider }: { provider: ProviderProfile }) {
  const updateProviderConfig = useProviderStore((state) => state.updateConfig);
  const loadProviders = useProviderStore((state) => state.load);
  const [draft, setDraft] = useState<Record<string, unknown>>({});
  const [errorMessage, setErrorMessage] = useState("");
  const [actionMessage, setActionMessage] = useState("");
  const draftRef = useRef<Record<string, unknown>>({});
  const saveTimersRef = useRef(new Map<string, number>());
  const configSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const configFields = provider.configFields || [];
  const apiKeyUrl = ASR_API_KEY_URLS[provider.id];
  const credentialLabel = configFields.find((field) => field.secret)?.label ?? "API Key";

  useEffect(() => {
    setDraft((current) => {
      const next = { ...(provider.config || {}) };
      for (const field of configFields) {
        if (field.secret && current[field.key]) next[field.key] = current[field.key];
      }
      draftRef.current = next;
      return next;
    });
  }, [provider.config]);

  useEffect(() => () => {
    for (const timer of saveTimersRef.current.values()) window.clearTimeout(timer);
    saveTimersRef.current.clear();
    draftRef.current = {};
  }, []);

  const queueConfigUpdate = (config: Record<string, unknown>): Promise<ProviderProfile> => {
    const operation = configSaveQueueRef.current.then(() => updateProviderConfig(provider.id, config));
    configSaveQueueRef.current = operation.then(() => undefined, () => undefined);
    return operation;
  };

  const persistField = async (field: NonNullable<ProviderProfile["configFields"]>[number], rawValue: unknown) => {
    if (field.secret && (typeof rawValue !== "string" || !rawValue.trim())) return;
    const value = field.fieldType === "number" && rawValue !== "" ? Number(rawValue) : rawValue;
    try {
      await queueConfigUpdate({ [field.key]: value });
      setErrorMessage("");
      if (field.secret && draftRef.current[field.key] === rawValue) {
        const next = { ...draftRef.current, [field.key]: "" };
        draftRef.current = next;
        setDraft(next);
      }
    } catch (error) {
      setErrorMessage(`自动保存失败：${String(error)}`);
    }
  };

  const updateField = (
    field: NonNullable<ProviderProfile["configFields"]>[number],
    value: unknown,
    immediate = false,
  ) => {
    const next = { ...draftRef.current, [field.key]: value };
    draftRef.current = next;
    setDraft(next);
    setErrorMessage("");
    const previousTimer = saveTimersRef.current.get(field.key);
    if (previousTimer !== undefined) window.clearTimeout(previousTimer);
    if (immediate) {
      saveTimersRef.current.delete(field.key);
      void persistField(field, value);
      return;
    }
    const timer = window.setTimeout(() => {
      saveTimersRef.current.delete(field.key);
      void persistField(field, draftRef.current[field.key]);
    }, 500);
    saveTimersRef.current.set(field.key, timer);
  };

  const runAction = async (action: string) => {
    if (
      ["openLogin", "syncSession", "clearSession"].includes(action) &&
      !window.confirm(`插件将执行“${PLUGIN_ACTION_LABELS[action] || action}”。是否继续？`)
    ) return;
    try {
      const result = await cmd<Record<string, unknown>>(CMD.runProviderPluginAction, {
        providerId: provider.id,
        action,
      });
      await loadProviders();
      setActionMessage(String(result.message || result.status || "操作完成。"));
    } catch (error) {
      setActionMessage(`操作失败：${String(error)}`);
    }
  };

  return (
    <Collapse
      title={provider.displayName}
      subtitle={providerConfigurationStatus(provider)}
    >
      <div className="flex flex-col gap-3">
        {apiKeyUrl && (
          <p className="text-xs text-[var(--color-fg-subtle)]">
            <ApiKeyLink
              url={apiKeyUrl}
              label={`点击此处获取 ${credentialLabel}`}
              onError={(error) => setErrorMessage(`打开密钥页面失败：${error}`)}
            />
          </p>
        )}
        {configFields.map((field) =>
          field.fieldType === "boolean" ? (
            <CheckField
              key={field.key}
              checked={Boolean(draft[field.key])}
              onChange={(value) => updateField(field, value, true)}
            >
              {field.label}
            </CheckField>
          ) : field.secret ? (
            <Field key={field.key} label={field.label}>
              <SecretInput
                id={`provider-secret-${provider.id}-${field.key}`}
                draftValue={String(draft[field.key] ?? "")}
                hasStoredValue={Boolean(provider.status?.hasApiKey)}
                placeholder={provider.status?.hasApiKey ? "已保存，留空表示不修改" : ""}
                onDraftChange={(value) => updateField(field, value)}
                onBlur={() => updateField(field, draftRef.current[field.key] ?? "", true)}
              />
            </Field>
          ) : (
            <Field key={field.key} label={field.label}>
              <Input
                type={field.fieldType === "number" ? "number" : "text"}
                value={String(draft[field.key] ?? "")}
                placeholder={field.secret && provider.status?.hasApiKey ? "已保存，留空表示不修改" : ""}
                onChange={(event) => updateField(field, event.target.value)}
                onBlur={() => updateField(field, draftRef.current[field.key] ?? "", true)}
              />
            </Field>
          ),
        )}
        <div className="flex flex-wrap gap-2">
          {(provider.actions || []).filter((action) => action !== "manageHotwords").map((action) => (
            <Button key={action} size="sm" onClick={() => void runAction(action)}>
              {PLUGIN_ACTION_LABELS[action] || action}
            </Button>
          ))}
        </div>
        {errorMessage && <p className="text-xs text-[var(--color-danger)]">{errorMessage}</p>}
        {actionMessage && <p className="text-xs text-[var(--color-fg-subtle)]">{actionMessage}</p>}
      </div>
    </Collapse>
  );
}

interface BailianAdvancedConfig {
  languageHints: string[];
  semanticPunctuationEnabled: boolean;
  maxSentenceSilence: number;
  multiThresholdModeEnabled: boolean;
  heartbeat: boolean;
  speechNoiseThreshold: string;
}

function bailianAdvancedConfig(config: Record<string, unknown> | undefined): BailianAdvancedConfig {
  return {
    languageHints: Array.isArray(config?.languageHints) ? config.languageHints as string[] : [],
    semanticPunctuationEnabled: Boolean(config?.semanticPunctuationEnabled),
    maxSentenceSilence: Number(config?.maxSentenceSilence ?? 1300),
    multiThresholdModeEnabled: Boolean(config?.multiThresholdModeEnabled),
    heartbeat: Boolean(config?.heartbeat),
    speechNoiseThreshold: config?.speechNoiseThreshold === null || config?.speechNoiseThreshold === undefined
      ? ""
      : String(config.speechNoiseThreshold),
  };
}

function BailianProviderConfig({ provider }: { provider: ProviderProfile }) {
  const updateProviderConfig = useProviderStore((state) => state.updateConfig);
  const [apiKey, setApiKey] = useState("");
  const [advanced, setAdvanced] = useState(() => bailianAdvancedConfig(provider.config));
  const [errorMessage, setErrorMessage] = useState("");

  const hasApiKey = !!provider.status?.hasApiKey;
  const apiKeyRef = useRef("");
  const advancedRef = useRef(advanced);
  const apiKeyTimerRef = useRef<number | null>(null);
  const advancedTimerRef = useRef<number | null>(null);
  const configSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => () => {
    if (apiKeyTimerRef.current !== null) window.clearTimeout(apiKeyTimerRef.current);
    if (advancedTimerRef.current !== null) window.clearTimeout(advancedTimerRef.current);
    apiKeyRef.current = "";
  }, []);

  const queueConfigUpdate = (config: Record<string, unknown>): Promise<ProviderProfile> => {
    const operation = configSaveQueueRef.current.then(() => updateProviderConfig(provider.id, config));
    configSaveQueueRef.current = operation.then(() => undefined, () => undefined);
    return operation;
  };

  const persistApiKey = async (value: string) => {
    const nextApiKey = value.trim();
    if (!nextApiKey) return;
    try {
      await queueConfigUpdate({ apiKey: nextApiKey });
      setErrorMessage("");
      if (apiKeyRef.current === value) {
        apiKeyRef.current = "";
        setApiKey("");
      }
    } catch (error) {
      setErrorMessage(`自动保存失败：${String(error)}`);
    }
  };

  const updateApiKey = (value: string, immediate = false) => {
    apiKeyRef.current = value;
    setApiKey(value);
    setErrorMessage("");
    if (apiKeyTimerRef.current !== null) window.clearTimeout(apiKeyTimerRef.current);
    if (immediate) {
      apiKeyTimerRef.current = null;
      void persistApiKey(value);
      return;
    }
    apiKeyTimerRef.current = window.setTimeout(() => {
      apiKeyTimerRef.current = null;
      void persistApiKey(apiKeyRef.current);
    }, 500);
  };

  const persistAdvanced = async (value: BailianAdvancedConfig) => {
    const threshold = value.speechNoiseThreshold.trim();
    try {
      await queueConfigUpdate({
        languageHints: value.languageHints,
        semanticPunctuationEnabled: value.semanticPunctuationEnabled,
        maxSentenceSilence: value.maxSentenceSilence,
        multiThresholdModeEnabled: value.multiThresholdModeEnabled,
        heartbeat: value.heartbeat,
        speechNoiseThreshold: threshold === "" ? null : Number(threshold),
      });
      setErrorMessage("");
    } catch (error) {
      setErrorMessage(`自动保存失败：${String(error)}`);
    }
  };

  const updateAdvanced = (patch: Partial<BailianAdvancedConfig>, immediate = false) => {
    const next = { ...advancedRef.current, ...patch };
    advancedRef.current = next;
    setAdvanced(next);
    setErrorMessage("");
    if (advancedTimerRef.current !== null) window.clearTimeout(advancedTimerRef.current);
    if (immediate) {
      advancedTimerRef.current = null;
      void persistAdvanced(next);
      return;
    }
    advancedTimerRef.current = window.setTimeout(() => {
      advancedTimerRef.current = null;
      void persistAdvanced(advancedRef.current);
    }, 500);
  };

  const toggleLanguageHint = (lang: string) => {
    const languageHints = advancedRef.current.languageHints;
    updateAdvanced({
      languageHints: languageHints.includes(lang)
        ? languageHints.filter((value) => value !== lang)
        : [...languageHints, lang],
    }, true);
  };

  return (
    <Collapse
      title={provider.displayName}
      subtitle={hasApiKey ? "已配置" : "需要填写"}
    >
      <p className="text-xs text-[var(--color-fg-subtle)]">
        <ApiKeyLink
          url={ASR_API_KEY_URLS.bailian}
          label="点击此处获取 API Key"
          onError={(error) => setErrorMessage(`打开密钥页面失败：${error}`)}
        />
      </p>

      <div className="mt-3">
        <SecretInput
          id="bailian-api-key"
          aria-label="阿里云百炼 API Key"
          draftValue={apiKey}
          hasStoredValue={hasApiKey}
          placeholder={hasApiKey ? "输入新 API Key 可覆盖当前配置" : "输入阿里云百炼 API Key"}
          onDraftChange={(value) => updateApiKey(value)}
          onBlur={() => updateApiKey(apiKeyRef.current, true)}
        />
      </div>

      <div className="mt-4 flex flex-col gap-3">
        <Collapse
          title="高级参数"
          className={NESTED_COLLAPSE_CLASS}
          headerClassName={NESTED_HEADER_CLASS}
          bodyClassName={NESTED_BODY_CLASS}
        >
          <div>
            <p className="text-xs text-[var(--color-fg-subtle)]">语种提示（language_hints）</p>
            <div className="mt-1.5 flex gap-4">
              {[
                { value: "zh", label: "中文" },
                { value: "en", label: "英文" },
                { value: "ja", label: "日语" },
              ].map((lang) => (
                <CheckField
                  key={lang.value}
                  checked={advanced.languageHints.includes(lang.value)}
                  onChange={() => toggleLanguageHint(lang.value)}
                >
                  {lang.label}
                </CheckField>
              ))}
            </div>
          </div>
          <CheckField
            className="mt-3"
            checked={advanced.semanticPunctuationEnabled}
            onChange={(value) => updateAdvanced({ semanticPunctuationEnabled: value }, true)}
          >
            语义断句（semantic_punctuation_enabled）
          </CheckField>
          <div className="mt-3">
            <Slider
              label="断句静音阈值"
              min={200}
              max={6000}
              step={100}
              value={advanced.maxSentenceSilence}
              format={(value) => `${value.toFixed(0)} ms`}
              onChange={(value) => updateAdvanced({ maxSentenceSilence: value })}
            />
          </div>
          <CheckField
            className="mt-3"
            checked={advanced.multiThresholdModeEnabled}
            onChange={(value) => updateAdvanced({ multiThresholdModeEnabled: value }, true)}
            disabled={advanced.semanticPunctuationEnabled}
          >
            多阈值模式（multi_threshold_mode_enabled，防止 VAD 断句切割过长，仅在语义断句关闭时生效）
          </CheckField>
          <CheckField
            className="mt-3"
            checked={advanced.heartbeat}
            onChange={(value) => updateAdvanced({ heartbeat: value }, true)}
          >
            心跳包（heartbeat，长时间静音保活连接）
          </CheckField>
          <Field label="噪音判定阈值（speech_noise_threshold，-1.0 ~ 1.0，留空使用默认）" className="mt-3">
            <Input
              type="number"
              min={-1}
              max={1}
              step={0.1}
              value={advanced.speechNoiseThreshold}
              onChange={(event) => updateAdvanced({ speechNoiseThreshold: event.target.value })}
              onBlur={() => updateAdvanced({ speechNoiseThreshold: advancedRef.current.speechNoiseThreshold }, true)}
            />
          </Field>
        </Collapse>
      </div>

      {errorMessage && <p className="mt-3 text-xs text-[var(--color-danger)]">{errorMessage}</p>}
    </Collapse>
  );
}

/** 不展开配置的条目：内置无配置项供应商，或配置入口在其他分区的多能力供应商。 */
function ProviderSummaryRow({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
      <span className="truncate text-sm font-medium text-[var(--color-fg)]">{title}</span>
      <span className="truncate text-xs text-[var(--color-fg-subtle)]">{subtitle}</span>
    </div>
  );
}

function ProviderSectionForCapability({ capability }: { capability: ProviderSectionCapability }) {
  const providers = useProviderStore((state) => state.profiles);

  const entries = providers.filter(
    (provider) => provider.enabled && provider.capabilities.includes(capability),
  );

  // 无配置项的供应商（系统内置 OCR、无配置字段的插件、本地模型包）不占据分区位置；
  // 多能力供应商仍在非主分区保留一行指引，避免用户以为该能力没有供应商。
  const renderEntry = (provider: ProviderProfile) => {
    const primary = primaryCapabilityOf(provider);
    if (primary !== capability) {
      return (
        <ProviderSummaryRow
          key={provider.id}
          title={provider.displayName}
          subtitle={`${providerConfigurationStatus(provider)} · 配置入口在「${SECTION_TITLES[primary]}」分区`}
        />
      );
    }
    if (provider.kind === "sdk:bailian") {
      return <BailianProviderConfig key={provider.id} provider={provider} />;
    }
    if (hasProviderConfiguration(provider)) {
      return <ProviderConfigEditor key={provider.id} provider={provider} />;
    }
    return null;
  };

  const rendered = entries.map(renderEntry).filter((entry) => entry !== null);

  return (
    <SettingsSection title={SECTION_TITLES[capability]}>
      {capability === "asr" && (
        <p className="text-xs text-[var(--color-fg-subtle)]">
          API Key 在应用私有目录中本地加密保存，不调用系统钥匙链。
        </p>
      )}
      {rendered.length > 0 ? (
        rendered
      ) : (
        <p className="text-xs text-[var(--color-fg-subtle)]">
          {entries.length === 0
            ? "暂无支持该能力的供应商，可通过「插件管理」安装。"
            : "当前已启用的供应商均无需配置。"}
        </p>
      )}
    </SettingsSection>
  );
}

export function SettingsProviderPanel() {
  const loadProviders = useProviderStore((s) => s.load);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  return (
    <>
      {SECTION_CAPABILITIES.map((capability) => (
        <ProviderSectionForCapability key={capability} capability={capability} />
      ))}
    </>
  );
}
