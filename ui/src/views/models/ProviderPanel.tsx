import { useEffect, useRef, useState } from "react";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { InspectorBody, InspectorHeader } from "@/components/layout/MasterDetail";
import { Button } from "@/components/ui/Button";
import { Collapse } from "@/components/ui/Collapse";
import { CheckField, Field } from "@/components/ui/Field";
import { Group, Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { IconButton } from "@/components/ui/IconButton";
import { Input, Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { RangeInput } from "@/components/ui/RangeInput";
import { SecretInput } from "@/components/ui/SecretInput";
import { Switch } from "@/components/ui/Switch";
import { useConfirm } from "@/components/ui/useConfirm";
import { asrModelDisplayLabel, asrModelMode, currentCatalog, useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { ApiKeyLink, providerApiKeyUrl } from "@/features/settings/apiKeyLinks";
import {
  llmModelShownByDefault,
  llmModelsFromProfile,
  manualLlmModel,
  REASONING_OPTIONS,
} from "@/features/models/llmModelOptions";
import { isModelVisible, modelVisibilityPatch, providerReady } from "@/features/models/providerModels";
import { TRANSLATION_MODEL_OPTIONS, TRANSLATION_PROVIDER_ID } from "@/features/translation/models";
import { CMD, cmd } from "@/lib/tauri";
import {
  useProviderStore,
  type LlmModelConfig,
  type LlmReasoningEffort,
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

const providerQueues = new Map<string, Promise<unknown>>();

/** 同一供应商的配置写入与模型列表刷新按顺序执行，避免整份写回的字段互相覆盖。 */
function enqueue<T>(providerId: string, task: () => Promise<T>): Promise<T> {
  const operation = (providerQueues.get(providerId) ?? Promise.resolve()).then(task);
  providerQueues.set(providerId, operation.then(() => undefined, () => undefined));
  return operation;
}

/** 写入时读取最新配置，再按它算出这次要写的字段。 */
function useConfigQueue(providerId: string) {
  const updateConfig = useProviderStore((state) => state.updateConfig);
  return (build: (latest: ProviderProfile) => Record<string, unknown>): Promise<ProviderProfile> => enqueue(providerId, () => {
    const latest = useProviderStore.getState().profiles.find((profile) => profile.id === providerId);
    if (!latest) throw new Error("供应商不存在");
    return updateConfig(providerId, build(latest));
  });
}

function ErrorLine({ text }: { text: string }) {
  return text ? <p role="alert" className="px-3.5 py-3 text-xs text-[var(--color-err)]">{text}</p> : null;
}

/** 密钥与连接设置：停止输入或失焦后自动保存，密钥保存后立即清掉页面里的明文。 */
function CredentialsGroup({ profile }: { profile: ProviderProfile }) {
  const { confirm, dialog } = useConfirm();
  const loadProviders = useProviderStore((state) => state.load);
  const queue = useConfigQueue(profile.id);
  const fields = [
    ...(profile.configFields ?? []).filter((field) => field.key !== "model"),
    ...(profile.kind === "llm:custom" ? [ENDPOINT_FIELD] : []),
  ];
  const [draft, setDraft] = useState<Record<string, unknown>>(() => ({ ...(profile.config ?? {}) }));
  const [error, setError] = useState("");
  const [actionMessage, setActionMessage] = useState("");
  const draftRef = useRef(draft);
  const timersRef = useRef(new Map<string, number>());
  const apiKeyUrl = providerApiKeyUrl(profile);
  const hasSecret = Boolean(profile.status?.hasApiKey);

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
      setActionMessage(String(result.message || result.status || "操作完成。"));
    } catch (reason) {
      setActionMessage(`操作失败：${String(reason)}`);
    }
  };

  // 只有 JavaScript 插件有可执行的操作；内置供应商的操作标记另有用途。
  const actions = profile.kind.startsWith("plugin:") ? (profile.actions ?? []).filter((action) => action in PLUGIN_ACTION_LABELS) : [];
  if (!fields.length && !actions.length) return null;

  return (
    <Group>
      {fields.map((field, index) => {
        const controlId = `provider-${profile.id}-${field.key}`;
        const link = index === 0 && apiKeyUrl
          ? <ApiKeyLink url={apiKeyUrl} label={`获取 ${field.label}`} onError={(reason) => setError(`打开密钥页面失败：${reason}`)} />
          : undefined;
        if (field.fieldType === "boolean") {
          return (
            <Row key={field.key} label={field.label} message={link}>
              <Switch checked={Boolean(draft[field.key])} onChange={(value) => update(field, value, true)} label={field.label} />
            </Row>
          );
        }
        return (
          <Row key={field.key} label={field.label} controlId={controlId} message={link}>
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
      {actionMessage && <p role="status" className="px-3.5 py-3 text-xs text-[var(--color-fg-subtle)]">{actionMessage}</p>}
      <ErrorLine text={error} />
      {dialog}
    </Group>
  );
}

interface CatalogModelRow {
  id: string;
  label: string;
}

/** 识别、翻译模型：每个模型一个开关，决定它是否出现在用途下拉框里。 */
function ModelSwitchGroup({ title, profile, models }: { title: string; profile: ProviderProfile; models: CatalogModelRow[] }) {
  const queue = useConfigQueue(profile.id);
  const [error, setError] = useState("");
  const ready = providerReady(profile);
  if (!models.length) return null;
  const toggle = (id: string, visible: boolean) => {
    setError("");
    queue((latest) => modelVisibilityPatch(latest, id, visible)).catch((reason) => setError(`保存失败：${String(reason)}`));
  };
  return (
    <Group title={title}>
      {models.map((model) => (
        <Row key={model.id} label={model.label} disabled={!ready}>
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
function LlmModelsGroup({ profile }: { profile: ProviderProfile }) {
  const refreshModels = useProviderStore((state) => state.refreshLlmModels);
  const defaults = useProviderStore((state) => state.defaults);
  const queue = useConfigQueue(profile.id);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState("");
  const [manualOpen, setManualOpen] = useState(false);
  const [manualName, setManualName] = useState("");
  const autoRefreshStartedRef = useRef(Boolean(profile.config?.modelListAttemptedAt));
  const ready = providerReady(profile);
  const isPlugin = profile.kind.startsWith("plugin:");
  const models = llmModelsFromProfile(profile);
  const visible = models.filter((model) => isModelVisible(profile, model.name, llmModelShownByDefault(profile, model)));
  const hidden = models.filter((model) => !visible.includes(model));

  const refresh = async () => {
    setRefreshing(true);
    setError("");
    try {
      await enqueue(profile.id, () => refreshModels(profile.id));
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

  const setReasoning = (name: string, reasoningEffort: LlmReasoningEffort) =>
    void saveModels((current) => current.map((model) => model.name === name ? { ...model, reasoningEffort } : model));

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

  const renderRow = (model: LlmModelConfig, shown: boolean) => (
    <Row
      key={model.name}
      label={<span className="flex items-center gap-2">{model.name}{model.source === "manual" && <span className="text-xs text-[var(--color-fg-faint)]">手动</span>}</span>}
      disabled={!ready}
      message={shown && model.availability === "missing" && <span className="text-[var(--color-warn)]">最新列表里没有这个模型，能否调用取决于供应商。</span>}
    >
      {shown && (
        <Select
          size="sm"
          className="w-28"
          aria-label={`${model.name} 推理强度`}
          value={model.reasoningEffort}
          disabled={!ready}
          onChange={(event) => setReasoning(model.name, event.target.value as LlmReasoningEffort)}
        >
          {REASONING_OPTIONS.map((option) => <option key={option.value} value={option.value}>推理 {option.label}</option>)}
        </Select>
      )}
      {model.source === "manual" && !isPlugin && (
        <IconButton size="sm" variant="dangerHover" label={`删除手动模型 ${model.name}`} onClick={() => removeManual(model.name)}>
          <Trash2 className="h-3.5 w-3.5" aria-hidden />
        </IconButton>
      )}
      <Switch checked={shown} disabled={!ready} onChange={(show) => toggle(model.name, show)} label={`在用途中显示 ${model.name}`} />
    </Row>
  );

  return (
    <Group
      title="大模型"
      right={!isPlugin && (
        <>
          <span className="text-xs text-[var(--color-fg-subtle)]">{formatTimestamp(profile.config?.modelsFetchedAt)}</span>
          <Button size="sm" disabled={!ready} onClick={() => setManualOpen(true)}><Plus className="h-3.5 w-3.5" aria-hidden />手动添加</Button>
          <Button size="sm" disabled={!ready || refreshing} onClick={() => void refresh()}>
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} aria-hidden />{refreshing ? "正在获取" : "刷新"}
          </Button>
        </>
      )}
    >
      {visible.map((model) => renderRow(model, true))}
      {!models.length && <Row label={ready ? "还没有模型，点「刷新」获取列表" : "填写密钥后可获取模型列表"} disabled />}
      {hidden.length > 0 && (
        <Collapse
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
  const queue = useConfigQueue(profile.id);
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
    .map((model) => ({ id: model.id, label: asrModelDisplayLabel(model) }));
  const translationModels = [
    ...(profile.id === TRANSLATION_PROVIDER_ID ? TRANSLATION_MODEL_OPTIONS.map((option) => ({ id: option.value, label: option.label })) : []),
    ...catalog.filter((model) => model.category === "translation").map((model) => ({ id: model.id, label: model.label })),
  ];
  const removable = profile.kind.startsWith("llm:") && profile.id !== "llm-groq";

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
        <ModelSwitchGroup title="识别模型" profile={profile} models={asrModels} />
        <ModelSwitchGroup title="翻译模型" profile={profile} models={translationModels} />
        {profile.capabilities.includes("llm") && <LlmModelsGroup profile={profile} />}
        {profile.kind === "sdk:bailian" && <BailianAdvancedSettings profile={profile} />}
      </InspectorBody>
      {dialog}
    </>
  );
}
