import { useEffect, useRef, useState } from "react";
import { Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { notify } from "@/store/useToastStore";
import { InspectorBody, InspectorHeader } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Collapse } from "@/components/ui/Collapse";
import { CheckField, Field } from "@/components/ui/Field";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { RangeInput } from "@/components/ui/RangeInput";
import { SecretInput } from "@/components/ui/SecretInput";
import { Tabs } from "@/components/ui/Tabs";
import { Switch } from "@/components/ui/Switch";
import { useConfirm } from "@/components/ui/useConfirm";
import {
  asrModelBaseLabel,
  asrModelMode,
  asrModelModeLabel,
  currentCatalog,
  sdkPricing,
  useModelCatalogRevision,
  type AsrModelMode,
} from "@/features/asr/modelRegistry";
import { llmPrice } from "@/features/models/llmModelOptions";
import { audioPrice, type ModelPrice } from "@/features/models/pricing";
import { adapterOf } from "@/features/models/recommendedLlmModels";
import { translationPrice } from "@/features/models/usage";
import { ApiKeyLink, providerApiKeyUrl } from "@/features/settings/apiKeyLinks";
import {
  llmModelShownByDefault,
  llmModelsFromProfile,
  manualLlmModel,
} from "@/features/models/llmModelOptions";
import { isModelVisible, modelVisibilityPatch, providerReady } from "@/features/models/providerModels";
import { isRecommendedLlmModel, llmModelLabel } from "@/features/models/recommendedLlmModels";
import { enqueueProviderTask, useProviderConfigQueue } from "@/features/models/providerConfigQueue";
import { TRANSLATION_MODEL_OPTIONS, TRANSLATION_PROVIDER_ID } from "@/features/translation/models";
import { CMD, cmd } from "@/lib/tauri";
import {
  useProviderStore,
  type LlmModelConfig,
  type ProviderProfile,
} from "@/store/useProviderStore";

type ConfigField = NonNullable<ProviderProfile["configFields"]>[number];

const PLUGIN_ACTION_LABELS: Record<string, string> = {
  openLogin: "打开登录窗口",
  syncSession: "获取并保护登录会话",
  clearSession: "清除登录会话",
  diagnose: "运行诊断",
};

const ENDPOINT_FIELD: ConfigField = { key: "endpoint", label: "接口地址", fieldType: "text", secret: false };

function ErrorLine({ text }: { text: string }) {
  return text ? <p role="alert" className="px-3.5 py-3 text-xs text-[var(--color-err)]">{text}</p> : null;
}

/** 密钥与连接设置：停止输入或失焦后自动保存，密钥保存后立即清掉页面里的明文。 */
function CredentialsGroup({ profile }: { profile: ProviderProfile }) {
  const { confirm, dialog } = useConfirm();
  const loadProviders = useProviderStore((state) => state.load);
  const queue = useProviderConfigQueue(profile.id);
  const fields = [
    ...(profile.configFields ?? []).filter((field) => field.key !== "model"),
    ...(profile.kind === "llm:custom" ? [ENDPOINT_FIELD] : []),
  ];
  const [draft, setDraft] = useState<Record<string, unknown>>(() => ({ ...(profile.config ?? {}) }));
  const [error, setError] = useState("");
  const draftRef = useRef(draft);
  const timersRef = useRef(new Map<string, number>());
  const configuredSecrets = profile.status?.configuredSecrets ?? [];

  useEffect(() => () => {
    for (const timer of timersRef.current.values()) window.clearTimeout(timer);
    draftRef.current = {};
  }, []);

  const persist = async (field: ConfigField, rawValue: unknown) => {
    if (field.secret && (typeof rawValue !== "string" || !rawValue.trim())) return;
    const value = field.fieldType === "number" && rawValue !== "" ? Number(rawValue) : typeof rawValue === "string" ? rawValue.trim() : rawValue;
    try {
      await queue(() => ({ [field.key]: value }));
      setError("");
      if (field.secret && draftRef.current[field.key] === rawValue) {
        draftRef.current = { ...draftRef.current, [field.key]: "" };
        setDraft(draftRef.current);
      }
    } catch (reason) {
      setError(`自动保存失败：${String(reason)}`);
    }
  };

  const update = (field: ConfigField, value: unknown, immediate = false) => {
    draftRef.current = { ...draftRef.current, [field.key]: value };
    setDraft(draftRef.current);
    const previous = timersRef.current.get(field.key);
    if (previous !== undefined) window.clearTimeout(previous);
    timersRef.current.delete(field.key);
    if (immediate) {
      void persist(field, value);
      return;
    }
    timersRef.current.set(field.key, window.setTimeout(() => {
      timersRef.current.delete(field.key);
      void persist(field, draftRef.current[field.key]);
    }, 500));
  };

  const runAction = async (action: string) => {
    if (
      ["openLogin", "syncSession", "clearSession"].includes(action)
      && !(await confirm({
        title: "确认执行插件操作",
        message: `插件将执行“${PLUGIN_ACTION_LABELS[action] || action}”。是否继续？`,
        confirmLabel: "继续",
        danger: action === "clearSession",
      }))
    ) {
      return;
    }
    try {
      const result = await cmd<Record<string, unknown>>(CMD.runProviderPluginAction, { providerId: profile.id, action });
      await loadProviders();
      notify(String(result.message || result.status || "操作完成"));
    } catch (reason) {
      setError(`操作失败：${String(reason)}`);
    }
  };

  // 只有 JavaScript 插件有可执行的操作；内置供应商的操作标记另有用途。
  const actions = profile.kind.startsWith("plugin:") ? (profile.actions ?? []).filter((action) => action in PLUGIN_ACTION_LABELS) : [];
  if (!fields.length && !actions.length) return null;

  return (
    <Group>
      {fields.map((field, index) => {
        const controlId = `provider-${profile.id}-${field.key}`;
        const keyUrl = providerApiKeyUrl(profile, field.key, index);
        const link = keyUrl
          ? <ApiKeyLink url={keyUrl} label="获取密钥" ariaLabel={`获取密钥：${field.label}`} onError={(reason) => setError(`打开密钥页面失败：${reason}`)} />
          : undefined;
        const hasSecret = configuredSecrets.includes(field.key);
        if (field.fieldType === "boolean") {
          return (
            <Row key={field.key} label={field.label} labelAside={link}>
              <Switch checked={Boolean(draft[field.key])} onChange={(value) => update(field, value, true)} label={field.label} />
            </Row>
          );
        }
        return (
          <Row key={field.key} label={field.label} controlId={controlId} labelAside={link}>
            <div className={ROW_CONTROL_W}>
              {field.secret ? (
                <SecretInput
                  id={controlId}
                  draftValue={String(draft[field.key] ?? "")}
                  hasStoredValue={hasSecret}
                  placeholder={hasSecret ? "已保存，输入新值可覆盖" : `粘贴 ${field.label}`}
                  onDraftChange={(value) => update(field, value)}
                  onBlur={() => update(field, draftRef.current[field.key] ?? "", true)}
                />
              ) : (
                <Input
                  id={controlId}
                  type={field.fieldType === "number" ? "number" : "text"}
                  value={String(draft[field.key] ?? "")}
                  placeholder={field.key === "endpoint" ? "https://example.com/v1/" : ""}
                  onChange={(event) => update(field, event.target.value)}
                  onBlur={() => update(field, draftRef.current[field.key] ?? "", true)}
                />
              )}
            </div>
          </Row>
        );
      })}
      {actions.length > 0 && (
        <div className="flex flex-wrap gap-2 px-3.5 py-3">
          {actions.map((action) => (
            <Button key={action} size="sm" onClick={() => void runAction(action)}>{PLUGIN_ACTION_LABELS[action] || action}</Button>
          ))}
        </div>
      )}
      <ErrorLine text={error} />
      {dialog}
    </Group>
  );
}

const PER_MILLION_TOKENS = "每百万 token 输入 / 输出";

/** 模型名称或 ID 包含搜索词；搜索词为空时都算匹配。 */
function matchesQuery(query: string, ...texts: string[]): boolean {
  const normalized = query.trim().toLocaleLowerCase();
  return !normalized || texts.some((text) => text.toLocaleLowerCase().includes(normalized));
}

interface CatalogModelRow {
  id: string;
  label: string;
  mode?: AsrModelMode;
  price?: ModelPrice;
}

/** 与模型选择面板一致：参考价和识别方式放在名称右侧，不写进名称。 */
function ModelMeta({ price, mode }: { price?: ModelPrice; mode?: AsrModelMode }) {
  return (
    <>
      {price && (
        <span
          className="text-xs tabular-nums text-[var(--color-fg-subtle)]"
          aria-label={price.perMillionTokens ? `每百万 token 输入 / 输出 ${price.text}` : undefined}
        >
          {price.text}
        </span>
      )}
      {mode && (
        <span className="rounded-full border border-[var(--color-line)] px-2 py-0.5 text-[11px] text-[var(--color-fg-subtle)]">
          {asrModelModeLabel(mode)}
        </span>
      )}
    </>
  );
}

type ModelCategory = "all" | "asr" | "translation" | "llm";

/** 识别、翻译模型：每个模型一个开关，决定它是否出现在用途下拉框里。 */
function ModelSwitchGroup({ title, capability, profile, models, query }: {
  title: string;
  capability: "asr" | "translation";
  profile: ProviderProfile;
  models: CatalogModelRow[];
  query: string;
}) {
  const queue = useProviderConfigQueue(profile.id);
  const [error, setError] = useState("");
  const ready = providerReady(profile, capability);
  const shown = models.filter((model) => matchesQuery(query, model.label, model.id));
  if (!shown.length) return null;
  const toggle = (id: string, visible: boolean) => {
    setError("");
    queue((latest) => modelVisibilityPatch(latest, id, visible)).catch((reason) => setError(`保存失败：${String(reason)}`));
  };
  return (
    <Group
      title={title}
      right={shown.some((model) => model.price?.perMillionTokens) && <span className="text-xs text-[var(--color-fg-subtle)]">{PER_MILLION_TOKENS}</span>}
    >
      {shown.map((model) => (
        <Row key={model.id} label={model.label} disabled={!ready}>
          <ModelMeta price={model.price} mode={model.mode} />
          <Switch
            checked={isModelVisible(profile, model.id, true)}
            disabled={!ready}
            onChange={(visible) => toggle(model.id, visible)}
            label={`在用途中显示 ${model.label}`}
          />
        </Row>
      ))}
      <ErrorLine text={error} />
    </Group>
  );
}

function formatTimestamp(value: unknown): string {
  return typeof value === "number" && Number.isFinite(value) ? new Date(value).toLocaleString() : "尚未获取";
}

/** 大模型列表：显示开关、推理强度、手动添加与刷新。列表通常很长，未显示的收起来。 */
function LlmModelsGroup({ profile, query }: { profile: ProviderProfile; query: string }) {
  const refreshModels = useProviderStore((state) => state.refreshLlmModels);
  const defaults = useProviderStore((state) => state.defaults);
  const queue = useProviderConfigQueue(profile.id);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState("");
  const [manualOpen, setManualOpen] = useState(false);
  const [manualName, setManualName] = useState("");
  const autoRefreshStartedRef = useRef(Boolean(profile.config?.modelListAttemptedAt));
  const ready = providerReady(profile, "llm");
  const isPlugin = profile.kind.startsWith("plugin:");
  const models = llmModelsFromProfile(profile);
  const matched = models.filter((model) => matchesQuery(query, llmModelLabel(profile, model.name), model.name));
  const visible = matched.filter((model) => isModelVisible(profile, model.name, llmModelShownByDefault(profile, model)));
  const hidden = matched.filter((model) => !visible.includes(model));

  const refresh = async () => {
    setRefreshing(true);
    setError("");
    try {
      await enqueueProviderTask(profile.id, () => refreshModels(profile.id));
    } catch (reason) {
      setError(`模型列表获取失败：${String(reason)}`);
    } finally {
      setRefreshing(false);
    }
  };

  // 第一次填好密钥后自动取一次模型列表；自定义接口要等地址填好。
  useEffect(() => {
    if (autoRefreshStartedRef.current || !ready || isPlugin) return;
    if (profile.kind === "llm:custom" && !/^https?:\/\//.test(String(profile.config?.endpoint ?? ""))) return;
    autoRefreshStartedRef.current = true;
    void refresh();
  }, [ready, profile.config?.endpoint]);

  const saveModels = (build: (current: LlmModelConfig[]) => LlmModelConfig[], extra?: (latest: ProviderProfile) => Record<string, unknown>) => {
    setError("");
    return queue((latest) => ({ models: build(llmModelsFromProfile(latest)), ...extra?.(latest) }))
      .catch((reason) => setError(`保存失败：${String(reason)}`));
  };

  const toggle = (name: string, show: boolean) => {
    setError("");
    queue((latest) => modelVisibilityPatch(latest, name, show)).catch((reason) => setError(`保存失败：${String(reason)}`));
  };

  const addManual = async () => {
    const name = manualName.trim();
    if (!name) return;
    await saveModels(
      (current) => current.some((model) => model.name === name) ? current : [...current, manualLlmModel(name)],
      (latest) => modelVisibilityPatch(latest, name, true),
    );
    setManualName("");
    setManualOpen(false);
  };

  const removeManual = (name: string) => {
    if (defaults.llm === profile.id && String(profile.config?.model ?? "") === name) {
      setError("这个模型正被用作通用大模型，请先在「用途」里换掉。");
      return;
    }
    void saveModels((current) => current.filter((model) => model.name !== name));
  };

  const renderRow = (model: LlmModelConfig, shown: boolean) => {
    const label = llmModelLabel(profile, model.name);
    // 推荐模型不提供删除：删了也会随远端列表回来，用显示开关隐藏即可。
    const userAdded = model.source === "manual" && !isRecommendedLlmModel(profile, model.name);
    return (
      <Row
        key={model.name}
        label={<span className="flex items-center gap-2">{label}{userAdded && <span className="text-xs text-[var(--color-fg-faint)]">手动</span>}</span>}
        disabled={!ready}
        message={shown && model.availability === "missing" && <span className="text-[var(--color-warn)]">最新列表里没有这个模型，能否调用取决于供应商。</span>}
      >
        <ModelMeta price={llmPrice(adapterOf(profile.kind), model.name)} />
        {userAdded && !isPlugin && (
          <IconButton size="sm" variant="dangerText" label={`删除手动模型 ${label}`} onClick={() => removeManual(model.name)}>
            <Trash2 className="h-3.5 w-3.5" aria-hidden />
          </IconButton>
        )}
        <Switch checked={shown} disabled={!ready} onChange={(show) => toggle(model.name, show)} label={`在用途中显示 ${label}`} />
      </Row>
    );
  };

  return (
    <Group
      title="大模型"
      right={!isPlugin && (
        <>
          <span className="text-xs text-[var(--color-fg-subtle)]">{PER_MILLION_TOKENS} · 列表更新于 {formatTimestamp(profile.config?.modelsFetchedAt)}</span>
          <Button size="sm" disabled={!ready} onClick={() => setManualOpen(true)}><Plus className="h-3.5 w-3.5" aria-hidden />手动添加</Button>
          <Button size="sm" disabled={!ready || refreshing} onClick={() => void refresh()}>
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} aria-hidden />{refreshing ? "正在获取" : "刷新"}
          </Button>
        </>
      )}
    >
      {visible.map((model) => renderRow(model, true))}
      {!models.length && <Row label={ready ? "还没有模型，点「刷新」获取列表" : "填写密钥后可获取模型列表"} disabled />}
      {models.length > 0 && !matched.length && <Row label="没有匹配的模型" disabled />}
      {hidden.length > 0 && (
        <Collapse
          key={query.trim() ? "searching" : "browsing"}
          defaultOpen={Boolean(query.trim())}
          title={`未显示的模型（${hidden.length}）`}
          className="rounded-none border-0 bg-transparent"
          headerClassName="min-h-[var(--row-h)] px-3.5 py-2.5 text-[13px] text-[var(--color-fg-muted)]"
          bodyClassName="px-0 py-0 [&>*+*]:border-t [&>*+*]:border-[var(--color-line)]"
        >
          {hidden.map((model) => renderRow(model, false))}
        </Collapse>
      )}
      <ErrorLine text={error} />
      <Modal open={manualOpen} onClose={() => setManualOpen(false)} title="手动添加模型" className="max-w-md">
        <div className="flex flex-col gap-4 p-5">
          <Field label="模型名称" hint="填写服务商提供的完整模型名称">
            <Input
              value={manualName}
              autoFocus
              placeholder="例如 qwen/qwen3.8-27b"
              onChange={(event) => setManualName(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") void addManual(); }}
            />
          </Field>
          <div className="flex justify-end gap-2">
            <Button onClick={() => setManualOpen(false)}>取消</Button>
            <Button variant="primary" disabled={!manualName.trim()} onClick={() => void addManual()}>添加</Button>
          </div>
        </div>
      </Modal>
    </Group>
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

/** 百炼实时识别的高级参数。 */
function BailianAdvancedSettings({ profile }: { profile: ProviderProfile }) {
  const queue = useProviderConfigQueue(profile.id);
  const [advanced, setAdvanced] = useState(() => bailianAdvancedConfig(profile.config));
  const [error, setError] = useState("");
  const advancedRef = useRef(advanced);
  const timerRef = useRef<number | null>(null);

  useEffect(() => () => {
    if (timerRef.current !== null) window.clearTimeout(timerRef.current);
  }, []);

  const persist = async (value: BailianAdvancedConfig) => {
    const threshold = value.speechNoiseThreshold.trim();
    try {
      await queue(() => ({
        languageHints: value.languageHints,
        semanticPunctuationEnabled: value.semanticPunctuationEnabled,
        maxSentenceSilence: value.maxSentenceSilence,
        multiThresholdModeEnabled: value.multiThresholdModeEnabled,
        heartbeat: value.heartbeat,
        speechNoiseThreshold: threshold === "" ? null : Number(threshold),
      }));
      setError("");
    } catch (reason) {
      setError(`自动保存失败：${String(reason)}`);
    }
  };

  const update = (patch: Partial<BailianAdvancedConfig>, immediate = false) => {
    advancedRef.current = { ...advancedRef.current, ...patch };
    setAdvanced(advancedRef.current);
    if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    timerRef.current = null;
    if (immediate) {
      void persist(advancedRef.current);
      return;
    }
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      void persist(advancedRef.current);
    }, 500);
  };

  const toggleLanguageHint = (lang: string) => {
    const hints = advancedRef.current.languageHints;
    update({ languageHints: hints.includes(lang) ? hints.filter((value) => value !== lang) : [...hints, lang] }, true);
  };

  return (
    <Collapse
      title="识别高级设置"
      headerClassName="min-h-[var(--row-h)] px-3.5 py-2.5 text-[13px]"
      bodyClassName="px-0 py-0 [&>*+*]:border-t [&>*+*]:border-[var(--color-line)]"
    >
      <Row label="语种提示">
        {[{ value: "zh", label: "中文" }, { value: "en", label: "英文" }, { value: "ja", label: "日语" }].map((lang) => (
          <CheckField key={lang.value} checked={advanced.languageHints.includes(lang.value)} onChange={() => toggleLanguageHint(lang.value)}>
            {lang.label}
          </CheckField>
        ))}
      </Row>
      <Row label="语义断句" hint="semantic_punctuation_enabled">
        <Switch checked={advanced.semanticPunctuationEnabled} onChange={(value) => update({ semanticPunctuationEnabled: value }, true)} label="语义断句" />
      </Row>
      <Row label="断句静音阈值">
        <RangeInput
          className="w-40"
          ariaLabel="断句静音阈值"
          min={200}
          max={6000}
          step={100}
          value={advanced.maxSentenceSilence}
          onChange={(value) => update({ maxSentenceSilence: value })}
        />
        <span className="w-16 text-right text-xs tabular-nums text-[var(--color-fg-muted)]">{advanced.maxSentenceSilence.toFixed(0)} ms</span>
      </Row>
      <Row
        label="多阈值模式"
        hint="multi_threshold_mode_enabled，防止 VAD 断句切割过长，仅在语义断句关闭时生效"
        disabled={advanced.semanticPunctuationEnabled}
      >
        <Switch
          checked={advanced.multiThresholdModeEnabled}
          onChange={(value) => update({ multiThresholdModeEnabled: value }, true)}
          disabled={advanced.semanticPunctuationEnabled}
          label="多阈值模式"
        />
      </Row>
      <Row label="心跳包" hint="heartbeat，长时间静音保活连接">
        <Switch checked={advanced.heartbeat} onChange={(value) => update({ heartbeat: value }, true)} label="心跳包" />
      </Row>
      <Row label="噪音判定阈值" hint="speech_noise_threshold，-1.0 ~ 1.0，留空使用默认" controlId="bailian-speech-noise-threshold">
        <Input
          id="bailian-speech-noise-threshold"
          className="w-32"
          type="number"
          min={-1}
          max={1}
          step={0.1}
          value={advanced.speechNoiseThreshold}
          onChange={(event) => update({ speechNoiseThreshold: event.target.value })}
          onBlur={() => update({ speechNoiseThreshold: advancedRef.current.speechNoiseThreshold }, true)}
        />
      </Row>
      <ErrorLine text={error} />
    </Collapse>
  );
}

/** 一个供应商的全部配置：密钥与设置，以及它提供的识别、翻译、大模型及其显示开关。 */
export function ProviderPanel({ profile }: { profile: ProviderProfile }) {
  useModelCatalogRevision();
  const { confirm, dialog } = useConfirm();
  const removeLlmProvider = useProviderStore((state) => state.removeLlmProvider);
  const [error, setError] = useState("");
  const catalog = currentCatalog().models.filter((model) => model.providerId === profile.id);
  const asrModels = catalog
    .filter((model) => asrModelMode(model))
    .map((model) => ({
      id: model.id,
      label: asrModelBaseLabel(model),
      mode: asrModelMode(model),
      price: audioPrice(model.capabilityId ? sdkPricing()?.capabilities[model.capabilityId] : undefined),
    }));
  const translationModels = [
    ...(profile.id === TRANSLATION_PROVIDER_ID ? TRANSLATION_MODEL_OPTIONS.map((option) => ({ id: option.value, label: option.label, price: translationPrice(option.value) })) : []),
    ...catalog.filter((model) => model.category === "translation").map((model) => ({ id: model.id, label: model.label })),
  ];
  const removable = profile.kind.startsWith("llm:") && profile.id !== "llm-groq";
  const hasLlm = profile.capabilities.includes("llm");
  const [category, setCategory] = useState<ModelCategory>("all");
  const [query, setQuery] = useState("");
  const categories = [
    { key: "asr" as const, label: "识别", count: asrModels.length },
    { key: "translation" as const, label: "翻译", count: translationModels.length },
    { key: "llm" as const, label: "大模型", count: hasLlm ? llmModelsFromProfile(profile).length : 0 },
  ].filter((item) => item.count > 0 || (item.key === "llm" && hasLlm));
  const showCategory = (key: ModelCategory) => category === "all" || category === key;

  const remove = async () => {
    if (!(await confirm({
      title: "确认删除供应商",
      message: `确定删除“${profile.displayName}”吗？已保存的配置会一并移除。`,
      confirmLabel: "删除",
      danger: true,
    }))) {
      return;
    }
    try {
      await removeLlmProvider(profile.id);
    } catch (reason) {
      setError(`删除失败：${String(reason)}`);
    }
  };

  return (
    <>
      <InspectorHeader
        title={profile.displayName}
        right={removable && <Button size="sm" variant="danger" onClick={() => void remove()}>删除供应商</Button>}
      />
      <InspectorBody>
        {error && <p role="alert" className="text-xs text-[var(--color-err)]">{error}</p>}
        <CredentialsGroup profile={profile} />
        {categories.length > 0 && (
          <div className="flex items-center gap-2">
            {categories.length > 1 && (
              <Tabs<ModelCategory>
                variant="view"
                ariaLabel="按模型类型筛选"
                tabs={[{ key: "all", label: "全部" }, ...categories.map((item) => ({ key: item.key, label: `${item.label} ${item.count}` }))]}
                active={category}
                onChange={setCategory}
              />
            )}
            <div className="relative min-w-0 flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--color-fg-faint)]" aria-hidden />
              <Input
                value={query}
                aria-label="搜索模型"
                placeholder="搜索模型"
                onChange={(event) => setQuery(event.target.value)}
                className="pl-9"
              />
            </div>
          </div>
        )}
        {showCategory("asr") && <ModelSwitchGroup title="识别模型" capability="asr" profile={profile} models={asrModels} query={query} />}
        {showCategory("translation") && <ModelSwitchGroup title="翻译模型" capability="translation" profile={profile} models={translationModels} query={query} />}
        {hasLlm && showCategory("llm") && <LlmModelsGroup profile={profile} query={query} />}
        {profile.kind === "sdk:bailian" && <BailianAdvancedSettings profile={profile} />}
      </InspectorBody>
      {dialog}
    </>
  );
}
