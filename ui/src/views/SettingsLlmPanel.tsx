import { useEffect, useState } from "react";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Collapse } from "@/components/ui/Collapse";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { SecretInput } from "@/components/ui/SecretInput";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, cmd } from "@/lib/tauri";
import {
  useProviderStore,
  type LlmModelAvailability,
  type LlmModelConfig,
  type LlmModelSource,
  type LlmReasoningEffort,
  type ProviderProfile,
} from "@/store/useProviderStore";

const PRESETS = [
  { adapter: "groq", name: "Groq", model: "openai/gpt-oss-20b" },
  { adapter: "openai", name: "OpenAI", model: "gpt-4o-mini" },
  { adapter: "anthropic", name: "Anthropic", model: "claude-haiku-4-5" },
  { adapter: "gemini", name: "Google Gemini", model: "gemini-2.5-flash" },
  { adapter: "deepseek", name: "DeepSeek", model: "deepseek-v4-flash" },
  { adapter: "open_router", name: "OpenRouter", model: "google/gemini-2.0-flash-001" },
  { adapter: "custom", name: "自定义 OpenAI 兼容接口", model: "" },
] as const;

const REASONING_OPTIONS: { value: LlmReasoningEffort; label: string }[] = [
  { value: "auto", label: "自动（供应商默认）" },
  { value: "zero", label: "关闭" },
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
];

function isSource(value: unknown): value is LlmModelSource {
  return value === "remote" || value === "manual";
}

function isAvailability(value: unknown): value is LlmModelAvailability {
  return value === "available" || value === "missing" || value === "unknown";
}

function isReasoningEffort(value: unknown): value is LlmReasoningEffort {
  return REASONING_OPTIONS.some((option) => option.value === value);
}

function manualModel(name: string): LlmModelConfig {
  return {
    name,
    source: "manual",
    availability: "unknown",
    reasoningEffort: "auto",
    temperature: 0.1,
    maxTokens: null,
  };
}

function modelsFromProfile(profile: ProviderProfile): LlmModelConfig[] {
  const rawModels = Array.isArray(profile.config?.models) ? profile.config.models : [];
  const models: LlmModelConfig[] = [];
  for (const value of rawModels) {
    if (!value || typeof value !== "object") continue;
    const raw = value as Record<string, unknown>;
    const name = typeof raw.name === "string" ? raw.name.trim() : "";
    if (!name || models.some((model) => model.name === name)) continue;
    models.push({
      name,
      source: isSource(raw.source) ? raw.source : "manual",
      availability: isAvailability(raw.availability) ? raw.availability : "unknown",
      reasoningEffort: isReasoningEffort(raw.reasoningEffort) ? raw.reasoningEffort : "auto",
      temperature: raw.temperature === null
        ? null
        : typeof raw.temperature === "number"
          ? raw.temperature
          : 0.1,
      maxTokens: raw.maxTokens === null || raw.maxTokens === undefined
        ? null
        : typeof raw.maxTokens === "number"
          ? raw.maxTokens
          : null,
    });
  }
  const current = String(profile.config?.model ?? "").trim();
  if (current && !models.some((model) => model.name === current)) models.push(manualModel(current));
  return models.sort((left, right) => left.name.localeCompare(right.name));
}

function modelLabel(model: LlmModelConfig): string {
  if (model.availability === "missing") return `${model.name}（最新列表中不可用）`;
  if (model.source === "manual") return `${model.name}（手动）`;
  return model.name;
}

function formatTimestamp(value: unknown): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "尚未获取";
  return new Date(value).toLocaleString();
}

function validateModels(models: LlmModelConfig[]): string | null {
  for (const model of models) {
    if (model.temperature !== null && (!Number.isFinite(model.temperature) || model.temperature < 0 || model.temperature > 2)) {
      return `“${model.name}”的温度必须在 0 到 2 之间`;
    }
    if (model.maxTokens !== null && (!Number.isInteger(model.maxTokens) || model.maxTokens <= 0)) {
      return `“${model.name}”的最大输出 Token 必须是正整数`;
    }
  }
  return null;
}

function LlmProfileEditor({ profile }: { profile: ProviderProfile }) {
  const updateConfig = useProviderStore((state) => state.updateConfig);
  const refreshModels = useProviderStore((state) => state.refreshLlmModels);
  const setDefault = useProviderStore((state) => state.setDefault);
  const remove = useProviderStore((state) => state.removeLlmProvider);
  const isDefault = profile.effectiveCapabilities?.includes("llm") ?? false;
  const isBuiltin = profile.id === "llm-groq";
  const isCustom = profile.kind === "llm:custom";
  const [model, setModel] = useState("");
  const [models, setModels] = useState<LlmModelConfig[]>([]);
  const [endpoint, setEndpoint] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [message, setMessage] = useState("");
  const [messageError, setMessageError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [manualModalOpen, setManualModalOpen] = useState(false);
  const [manualName, setManualName] = useState("");

  useEffect(() => {
    setModel(String(profile.config?.model ?? ""));
    setModels(modelsFromProfile(profile));
    setEndpoint(String(profile.config?.endpoint ?? ""));
    setApiKey("");
  }, [profile.id, profile.config]);

  const selectedModel = models.find((item) => item.name === model);
  const manualModels = models.filter((item) => item.source === "manual");

  const updateSelectedModel = (patch: Partial<LlmModelConfig>) => {
    setModels((current) => current.map((item) => item.name === model ? { ...item, ...patch } : item));
  };

  const persist = async (
    nextModel = model,
    nextModels = models,
  ): Promise<ProviderProfile> => {
    const validationError = validateModels(nextModels);
    if (validationError) throw new Error(validationError);
    const config: Record<string, unknown> = {
      model: nextModel.trim(),
      models: nextModels,
    };
    if (isCustom) config.endpoint = endpoint.trim();
    if (apiKey.trim()) config.apiKey = apiKey.trim();
    const updated = await updateConfig(profile.id, config);
    setApiKey("");
    return updated;
  };

  const save = async () => {
    try {
      const shouldAutoRefresh = !profile.config?.modelListAttemptedAt
        && Boolean(apiKey.trim() || profile.status?.hasApiKey);
      await persist();
      if (shouldAutoRefresh) {
        setRefreshing(true);
        try {
          await refreshModels(profile.id);
          setMessage("配置已保存，并已自动获取模型列表。");
        } catch (error) {
          setMessage(`配置已保存，首次获取模型失败：${String(error)}`);
          setMessageError(true);
          return;
        } finally {
          setRefreshing(false);
        }
      } else {
        setMessage("配置已保存。");
      }
      setMessageError(false);
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
      setMessageError(true);
    }
  };

  const refresh = async () => {
    setRefreshing(true);
    try {
      await persist();
      const updated = await refreshModels(profile.id);
      const count = modelsFromProfile(updated).length;
      setMessage(`模型列表已更新，共 ${count} 个模型。`);
      setMessageError(false);
    } catch (error) {
      setMessage(`模型列表更新失败：${String(error)}`);
      setMessageError(true);
    } finally {
      setRefreshing(false);
    }
  };

  const addManualModel = async () => {
    const name = manualName.trim();
    if (!name) {
      setMessage("请输入模型名称。");
      setMessageError(true);
      return;
    }
    const nextModels = models.some((item) => item.name === name)
      ? models
      : [...models, manualModel(name)].sort((left, right) => left.name.localeCompare(right.name));
    try {
      await persist(name, nextModels);
      setModel(name);
      setModels(nextModels);
      setManualName("");
      setManualModalOpen(false);
      setMessage("手动模型已添加并设为当前模型。");
      setMessageError(false);
    } catch (error) {
      setMessage(`添加模型失败：${String(error)}`);
      setMessageError(true);
    }
  };

  const deleteManualModel = async (name: string) => {
    if (name === model) {
      setMessage("请先切换到其他模型，再删除当前模型。");
      setMessageError(true);
      return;
    }
    const nextModels = models.filter((item) => item.name !== name);
    try {
      await persist(model, nextModels);
      setModels(nextModels);
      setMessage(`已删除手动模型“${name}”。`);
      setMessageError(false);
    } catch (error) {
      setMessage(`删除模型失败：${String(error)}`);
      setMessageError(true);
    }
  };

  const deleteProfile = async () => {
    if (!window.confirm(`确定删除“${profile.displayName}”吗？`)) return;
    try {
      await remove(profile.id);
    } catch (error) {
      setMessage(`删除失败：${String(error)}`);
      setMessageError(true);
    }
  };

  return (
    <Collapse
      title={profile.displayName}
      subtitle={`${isDefault ? "默认 · " : ""}${profile.status?.hasApiKey ? "已配置" : "未配置"} · ${models.length} 个模型`}
      defaultOpen={isDefault}
    >
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field
          label="API Key"
          controlId={`llm-api-key-${profile.id}`}
          hint={profile.status?.hasApiKey ? "已保存；留空表示不修改" : undefined}
        >
          <SecretInput
            id={`llm-api-key-${profile.id}`}
            draftValue={apiKey}
            hasStoredValue={Boolean(profile.status?.hasApiKey)}
            placeholder={profile.status?.hasApiKey ? "输入新 Key 可覆盖" : "输入 API Key"}
            onDraftChange={setApiKey}
            revealStoredValue={() => cmd<string>(CMD.getProviderApiKey, { providerId: profile.id })}
            onRevealError={(error) => {
              setMessage(`读取 API Key 失败：${String(error)}`);
              setMessageError(true);
            }}
          />
        </Field>
        {isCustom && (
          <Field
            label="接口地址"
            hint="OpenAI 兼容接口的基础地址，例如 https://example.com/v1/"
          >
            <Input
              value={endpoint}
              placeholder="https://example.com/v1/"
              onChange={(event) => setEndpoint(event.target.value)}
            />
          </Field>
        )}
      </div>

      <div className="mt-5 border-t border-[var(--color-line)] pt-4">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div>
            <h4 className="text-sm font-medium text-[var(--color-fg)]">模型</h4>
            <p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">
              {models.length} 个模型 · 最后更新：{formatTimestamp(profile.config?.modelsFetchedAt)}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => setManualModalOpen(true)}>
              <Plus className="h-3.5 w-3.5" aria-hidden />手动添加
            </Button>
            <Button size="sm" onClick={refresh} disabled={refreshing}>
              <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} aria-hidden />
              {refreshing ? "正在获取" : "刷新模型"}
            </Button>
          </div>
        </div>

        <Field
          label="当前模型"
          controlId={`llm-model-${profile.id}`}
          hint="智能文本处理会使用这个模型；下拉列表支持搜索"
        >
          <Select
            id={`llm-model-${profile.id}`}
            value={model}
            searchable
            searchPlaceholder="搜索模型…"
            onChange={(event) => setModel(event.target.value)}
          >
            {models.map((item) => <option key={item.name} value={item.name}>{modelLabel(item)}</option>)}
          </Select>
        </Field>

        {selectedModel && (
          <div className="mt-3 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface-hover)] p-3">
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <Field label="推理强度" hint="不支持时供应商会返回错误">
                <Select
                  value={selectedModel.reasoningEffort}
                  onChange={(event) => updateSelectedModel({ reasoningEffort: event.target.value as LlmReasoningEffort })}
                >
                  {REASONING_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                </Select>
              </Field>
              <Field label="温度" hint="留空使用供应商默认值">
                <Input
                  type="number"
                  min="0"
                  max="2"
                  step="0.1"
                  value={selectedModel.temperature ?? ""}
                  placeholder="默认"
                  onChange={(event) => updateSelectedModel({
                    temperature: event.target.value === "" ? null : Number(event.target.value),
                  })}
                />
              </Field>
              <Field label="最大输出 Token" hint="留空表示不限制">
                <Input
                  type="number"
                  min="1"
                  step="1"
                  value={selectedModel.maxTokens ?? ""}
                  placeholder="默认"
                  onChange={(event) => updateSelectedModel({
                    maxTokens: event.target.value === "" ? null : Number(event.target.value),
                  })}
                />
              </Field>
            </div>
            {selectedModel.availability === "missing" && (
              <p className="mt-3 text-xs text-[var(--color-warn)]">
                该模型未出现在最新接口结果中，配置已保留；调用是否可用取决于供应商。
              </p>
            )}
          </div>
        )}

        {manualModels.length > 0 && (
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <span className="text-xs text-[var(--color-fg-subtle)]">手动模型</span>
            {manualModels.map((item) => (
              <span
                key={item.name}
                className="inline-flex max-w-full items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--color-line)] bg-[var(--color-surface)] px-2 py-1 text-xs text-[var(--color-fg-muted)]"
              >
                <span className="max-w-52 truncate">{item.name}</span>
                <button
                  type="button"
                  className="rounded p-0.5 text-[var(--color-fg-subtle)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-err)]"
                  aria-label={`删除手动模型 ${item.name}`}
                  onClick={() => void deleteManualModel(item.name)}
                >
                  <Trash2 className="h-3 w-3" aria-hidden />
                </button>
              </span>
            ))}
          </div>
        )}
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        <Button size="sm" variant="primary" onClick={save} disabled={refreshing}>保存</Button>
        {!isDefault && <Button size="sm" onClick={() => void setDefault("llm", profile.id)}>设为默认</Button>}
        {!isBuiltin && <Button size="sm" variant="danger" onClick={deleteProfile}>删除供应商</Button>}
      </div>
      {message && (
        <p className={`mt-2 text-xs ${messageError ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]"}`}>
          {message}
        </p>
      )}

      <Modal open={manualModalOpen} onClose={() => setManualModalOpen(false)} title="手动添加模型" className="max-w-md">
        <div className="flex flex-col gap-4 p-5">
          <Field label="模型名称" hint="填写供应商接受的完整模型 ID">
            <Input
              value={manualName}
              placeholder="例如 openai/gpt-oss-20b"
              onChange={(event) => setManualName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void addManualModel();
              }}
            />
          </Field>
          <div className="flex justify-end gap-2">
            <Button onClick={() => setManualModalOpen(false)}>取消</Button>
            <Button variant="primary" onClick={addManualModel}>添加</Button>
          </div>
        </div>
      </Modal>
    </Collapse>
  );
}

export function SettingsLlmPanel() {
  const profiles = useProviderStore((state) => state.profiles).filter((profile) => profile.kind.startsWith("llm:"));
  const defaults = useProviderStore((state) => state.defaults);
  const setDefault = useProviderStore((state) => state.setDefault);
  const add = useProviderStore((state) => state.addLlmProvider);
  const refreshModels = useProviderStore((state) => state.refreshLlmModels);
  const [open, setOpen] = useState(false);
  const [adapter, setAdapter] = useState("groq");
  const [displayName, setDisplayName] = useState("Groq");
  const [model, setModel] = useState("openai/gpt-oss-20b");
  const [apiKey, setApiKey] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [message, setMessage] = useState("");
  const [panelMessage, setPanelMessage] = useState("");
  const [panelMessageError, setPanelMessageError] = useState(false);
  const [adding, setAdding] = useState(false);

  const selectPreset = (nextAdapter: string) => {
    const preset = PRESETS.find((item) => item.adapter === nextAdapter) ?? PRESETS[0];
    setAdapter(preset.adapter);
    setDisplayName(preset.name);
    setModel(preset.model);
    setEndpoint("");
  };

  const submit = async () => {
    setAdding(true);
    try {
      const profile = await add({ adapter, displayName, model, apiKey, endpoint });
      setOpen(false);
      setMessage("");
      setPanelMessage("大语言模型供应商已添加。");
      setPanelMessageError(false);
      const shouldAutoRefresh = Boolean(apiKey.trim())
        && (adapter !== "custom" || /^https?:\/\//.test(endpoint.trim()));
      setApiKey("");
      if (shouldAutoRefresh) {
        try {
          const updated = await refreshModels(profile.id);
          setPanelMessage(`供应商已添加，并获取到 ${modelsFromProfile(updated).length} 个模型。`);
          setPanelMessageError(false);
        } catch (error) {
          setPanelMessage(`供应商已添加，首次获取模型失败：${String(error)}`);
          setPanelMessageError(true);
        }
      }
    } catch (error) {
      setMessage(`添加失败：${String(error)}`);
    } finally {
      setAdding(false);
    }
  };

  return (
    <SettingsSection title="大语言模型">
      <Field
        label="默认模型供应商"
        controlId="default-llm-provider"
        hint="智能文本处理会使用这里选中的供应商及其当前模型"
        actions={<Button variant="primary" onClick={() => setOpen(true)}>+ 添加</Button>}
      >
        <Select id="default-llm-provider" value={defaults.llm} onChange={(event) => void setDefault("llm", event.target.value)}>
          {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.displayName}</option>)}
        </Select>
      </Field>

      {panelMessage && (
        <p className={`text-xs ${panelMessageError ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]"}`}>
          {panelMessage}
        </p>
      )}

      <div className="mt-3 flex flex-col gap-3">
        {profiles.map((profile) => <LlmProfileEditor key={profile.id} profile={profile} />)}
      </div>

      <Modal open={open} onClose={() => setOpen(false)} title="添加大语言模型" className="max-w-xl">
        <div className="flex flex-col gap-4 p-5">
          <Field label="快速选择">
            <Select value={adapter} onChange={(event) => selectPreset(event.target.value)}>
              {PRESETS.map((preset) => <option key={preset.adapter} value={preset.adapter}>{preset.name}</option>)}
            </Select>
          </Field>
          <Field label="显示名称">
            <Input value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
          </Field>
          <Field label="初始模型" hint="可留空，首次获取模型列表后会自动选择一个模型">
            <Input value={model} placeholder="可选的模型名称" onChange={(event) => setModel(event.target.value)} />
          </Field>
          {adapter === "custom" && (
            <Field label="接口地址" hint="OpenAI 兼容接口的基础地址">
              <Input value={endpoint} placeholder="https://example.com/v1/" onChange={(event) => setEndpoint(event.target.value)} />
            </Field>
          )}
          <Field label="API Key" hint="填写后会在添加完成时自动获取一次模型列表，也可以稍后配置">
            <Input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} />
          </Field>
          {message && <p className="text-xs text-[var(--color-err)]">{message}</p>}
          <div className="flex justify-end gap-2">
            <Button onClick={() => setOpen(false)} disabled={adding}>取消</Button>
            <Button variant="primary" onClick={submit} disabled={adding}>{adding ? "正在添加" : "添加"}</Button>
          </div>
        </div>
      </Modal>
    </SettingsSection>
  );
}
