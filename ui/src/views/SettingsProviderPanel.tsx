import { useEffect, useRef, useState } from "react";
import { Collapse } from "@/components/ui/Collapse";
import { Button } from "@/components/ui/Button";
import { Field, CheckField } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { SecretInput } from "@/components/ui/SecretInput";
import { Slider } from "@/components/ui/Slider";
import { SettingsSection } from "@/components/ui/SettingsSection";
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
  return provider.status?.configured || provider.status?.hasApiKey ? "已配置" : "未配置";
}

function hasPluginConfiguration(provider: ProviderProfile) {
  return Boolean(
    provider.configFields?.length ||
    provider.actions?.length ||
    provider.capabilities.includes("customization"),
  );
}

function PluginProviderConfig({ provider }: { provider: ProviderProfile }) {
  const updateProviderConfig = useProviderStore((state) => state.updateConfig);
  const loadProviders = useProviderStore((state) => state.load);
  const [draft, setDraft] = useState<Record<string, unknown>>({});
  const [message, setMessage] = useState("");
  const configFields = provider.configFields || [];

  useEffect(() => setDraft(provider.config || {}), [provider.config]);

  const save = async () => {
    const patch: Record<string, unknown> = {};
    for (const field of configFields) {
      const value = draft[field.key];
      if (field.secret && (value === undefined || value === "")) continue;
      patch[field.key] = field.fieldType === "number" && value !== "" ? Number(value) : value;
    }
    try {
      await updateProviderConfig(provider.id, patch);
      setMessage("插件配置已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
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
      setMessage(String(result.message || result.status || "操作完成。"));
    } catch (error) {
      setMessage(`操作失败：${String(error)}`);
    }
  };

  return (
    <Collapse
      title={provider.displayName}
      subtitle={providerConfigurationStatus(provider)}
    >
      <div className="flex flex-col gap-3">
        {configFields.map((field) =>
          field.fieldType === "boolean" ? (
            <CheckField
              key={field.key}
              checked={Boolean(draft[field.key])}
              onChange={(value) => setDraft((current) => ({ ...current, [field.key]: value }))}
            >
              {field.label}
            </CheckField>
          ) : (
            <Field key={field.key} label={field.label}>
              <Input
                type={field.secret ? "password" : field.fieldType === "number" ? "number" : "text"}
                value={String(draft[field.key] ?? "")}
                placeholder={field.secret && provider.status?.hasApiKey ? "已保存，留空表示不修改" : ""}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, [field.key]: event.target.value }))
                }
              />
            </Field>
          ),
        )}
        <div className="flex flex-wrap gap-2">
          {configFields.length > 0 && <Button size="sm" onClick={save}>保存插件配置</Button>}
          {(provider.actions || []).filter((action) => action !== "manageHotwords").map((action) => (
            <Button key={action} size="sm" onClick={() => void runAction(action)}>
              {PLUGIN_ACTION_LABELS[action] || action}
            </Button>
          ))}
        </div>
        {message && <p className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
      </div>
    </Collapse>
  );
}

function FunAsrProviderConfig({ provider }: { provider: ProviderProfile }) {
  const providerStatus = useProviderStore((state) => state.statusText);
  const updateProviderConfig = useProviderStore((state) => state.updateConfig);
  const [apiKey, setApiKey] = useState("");
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [apiKeySaving, setApiKeySaving] = useState(false);
  const [message, setMessage] = useState("");
  const [languageHints, setLanguageHints] = useState<string[]>([]);
  const [semanticPunctuation, setSemanticPunctuation] = useState(false);
  const [maxSentenceSilence, setMaxSentenceSilence] = useState(1300);
  const [multiThresholdMode, setMultiThresholdMode] = useState(false);
  const [heartbeat, setHeartbeat] = useState(false);
  const [noiseThreshold, setNoiseThreshold] = useState("");

  const hasApiKey = !!provider.status?.hasApiKey;
  const saveRequestRef = useRef(0);

  useEffect(() => {
    const config = provider.config;
    if (!config) return;
    setLanguageHints(Array.isArray(config.languageHints) ? (config.languageHints as string[]) : []);
    setSemanticPunctuation(!!config.semanticPunctuationEnabled);
    setMaxSentenceSilence(Number(config.maxSentenceSilence ?? 1300));
    setMultiThresholdMode(!!config.multiThresholdModeEnabled);
    setHeartbeat(!!config.heartbeat);
    setNoiseThreshold(
      config.speechNoiseThreshold === null || config.speechNoiseThreshold === undefined
        ? ""
        : String(config.speechNoiseThreshold),
    );
  }, [provider.config]);

  useEffect(() => {
    if (!apiKeyDirty) return;
    const nextApiKey = apiKey.trim();
    if (!nextApiKey) {
      setApiKeySaving(false);
      return;
    }

    setApiKeySaving(true);
    const timer = window.setTimeout(async () => {
      const requestId = saveRequestRef.current + 1;
      saveRequestRef.current = requestId;

      try {
        await updateProviderConfig(provider.id, { apiKey: nextApiKey });
        if (saveRequestRef.current !== requestId) return;
        setApiKeyDirty(false);
        setApiKey("");
        setMessage("API Key 已自动保存。");
      } catch (error) {
        if (saveRequestRef.current === requestId) setMessage(`保存失败：${String(error)}`);
      } finally {
        if (saveRequestRef.current === requestId) setApiKeySaving(false);
      }
    }, 500);

    return () => window.clearTimeout(timer);
  }, [apiKey, apiKeyDirty, updateProviderConfig, provider.id]);

  const openApiKeyPage = async () => {
    try {
      await cmd(CMD.openApiKeyPage);
    } catch (error) {
      setMessage(`打开链接失败：${String(error)}`);
    }
  };

  const toggleLanguageHint = (lang: string) => {
    setLanguageHints((prev) =>
      prev.includes(lang) ? prev.filter((value) => value !== lang) : [...prev, lang],
    );
  };

  const saveAdvanced = async () => {
    try {
      const threshold = noiseThreshold.trim();
      await updateProviderConfig(provider.id, {
        languageHints,
        semanticPunctuationEnabled: semanticPunctuation,
        maxSentenceSilence,
        multiThresholdModeEnabled: multiThresholdMode,
        heartbeat,
        speechNoiseThreshold: threshold === "" ? null : Number(threshold),
      });
      setMessage("高级参数已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
  };

  return (
    <Collapse
      title={provider.displayName}
      subtitle={hasApiKey ? "已配置" : "未配置"}
      defaultOpen
    >
      <p className="text-xs text-[var(--color-fg-subtle)]">
        <button
          type="button"
          onClick={openApiKeyPage}
          className="text-[var(--color-accent-light)] underline-offset-4 hover:underline"
        >
          点击此处获取 API Key
        </button>
      </p>

      <div className="mt-3">
        <SecretInput
          id="funasr-api-key"
          aria-label="阿里云百炼 API Key"
          draftValue={apiKey}
          hasStoredValue={hasApiKey}
          placeholder={hasApiKey ? "输入新 API Key 可覆盖当前配置" : "输入阿里云百炼 API Key"}
          onDraftChange={(value) => {
            setApiKey(value);
            setApiKeyDirty(true);
          }}
          revealStoredValue={() => cmd<string>(CMD.getProviderApiKey, { providerId: provider.id })}
          onRevealError={(error) => setMessage(`读取 API Key 失败：${String(error)}`)}
        />
      </div>
      <p className="mt-2 text-xs text-[var(--color-fg-subtle)]">
        当前状态：{hasApiKey ? "已配置" : "未配置"}
        {apiKeySaving ? " · 正在自动保存..." : providerStatus ? ` · ${providerStatus}` : ""}
      </p>

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
                  checked={languageHints.includes(lang.value)}
                  onChange={() => toggleLanguageHint(lang.value)}
                >
                  {lang.label}
                </CheckField>
              ))}
            </div>
          </div>
          <CheckField
            className="mt-3"
            checked={semanticPunctuation}
            onChange={setSemanticPunctuation}
          >
            语义断句（semantic_punctuation_enabled）
          </CheckField>
          <div className="mt-3">
            <Slider
              label="断句静音阈值"
              min={200}
              max={6000}
              step={100}
              value={maxSentenceSilence}
              format={(value) => `${value.toFixed(0)} ms`}
              onChange={setMaxSentenceSilence}
            />
          </div>
          <CheckField
            className="mt-3"
            checked={multiThresholdMode}
            onChange={setMultiThresholdMode}
            disabled={semanticPunctuation}
          >
            多阈值模式（multi_threshold_mode_enabled，防止 VAD 断句切割过长，仅在语义断句关闭时生效）
          </CheckField>
          <CheckField className="mt-3" checked={heartbeat} onChange={setHeartbeat}>
            心跳包（heartbeat，长时间静音保活连接）
          </CheckField>
          <Field label="噪音判定阈值（speech_noise_threshold，-1.0 ~ 1.0，留空使用默认）" className="mt-3">
            <Input
              type="number"
              min={-1}
              max={1}
              step={0.1}
              value={noiseThreshold}
              onChange={(event) => setNoiseThreshold(event.target.value)}
            />
          </Field>
          <Button size="sm" className="mt-3" onClick={saveAdvanced}>
            保存高级参数
          </Button>
        </Collapse>
      </div>

      {message && <p className="mt-3 text-xs text-[var(--color-fg-subtle)]">{message}</p>}
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
    if (provider.kind === "alibabacloud-funasr") {
      return <FunAsrProviderConfig key={provider.id} provider={provider} />;
    }
    if (provider.kind.startsWith("plugin:")) {
      if (!hasPluginConfiguration(provider)) {
        return (
          <ProviderSummaryRow
            key={provider.id}
            title={provider.displayName}
            subtitle="无需配置"
          />
        );
      }
      return <PluginProviderConfig key={provider.id} provider={provider} />;
    }
    // 内置或模型包供应商没有在线凭据配置项。
    return (
      <ProviderSummaryRow
        key={provider.id}
        title={provider.displayName}
        subtitle={provider.kind.startsWith("builtin-") ? "系统内置，无需配置" : "无需配置"}
      />
    );
  };

  return (
    <SettingsSection title={SECTION_TITLES[capability]}>
      {entries.length === 0 ? (
        <p className="text-xs text-[var(--color-fg-subtle)]">暂无支持该能力的供应商，可通过「插件管理」安装。</p>
      ) : (
        entries.map(renderEntry)
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
