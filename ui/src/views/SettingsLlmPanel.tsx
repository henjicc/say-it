import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Collapse } from "@/components/ui/Collapse";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { SecretInput } from "@/components/ui/SecretInput";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore, type ProviderProfile } from "@/store/useProviderStore";

const PRESETS = [
  { adapter: "groq", name: "Groq", model: "openai/gpt-oss-20b" },
  { adapter: "openai", name: "OpenAI", model: "gpt-4o-mini" },
  { adapter: "anthropic", name: "Anthropic", model: "claude-haiku-4-5" },
  { adapter: "gemini", name: "Google Gemini", model: "gemini-2.5-flash" },
  { adapter: "deepseek", name: "DeepSeek", model: "deepseek-v4-flash" },
  { adapter: "open_router", name: "OpenRouter", model: "google/gemini-2.0-flash-001" },
  { adapter: "custom", name: "自定义 OpenAI 兼容接口", model: "" },
] as const;

function LlmProfileEditor({ profile }: { profile: ProviderProfile }) {
  const updateConfig = useProviderStore((state) => state.updateConfig);
  const setDefault = useProviderStore((state) => state.setDefault);
  const remove = useProviderStore((state) => state.removeLlmProvider);
  const isDefault = profile.effectiveCapabilities?.includes("llm") ?? false;
  const isBuiltin = profile.id === "llm-groq";
  const isCustom = profile.kind === "llm:custom";
  const [model, setModel] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    setModel(String(profile.config?.model ?? ""));
    setEndpoint(String(profile.config?.endpoint ?? ""));
    setApiKey("");
  }, [profile.id, profile.config]);

  const save = async () => {
    try {
      const config: Record<string, unknown> = { model: model.trim() };
      if (isCustom) config.endpoint = endpoint.trim();
      if (apiKey.trim()) config.apiKey = apiKey.trim();
      await updateConfig(profile.id, config);
      setApiKey("");
      setMessage("配置已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
  };

  const deleteProfile = async () => {
    if (!window.confirm(`确定删除“${profile.displayName}”吗？`)) return;
    try {
      await remove(profile.id);
    } catch (error) {
      setMessage(`删除失败：${String(error)}`);
    }
  };

  return (
    <Collapse
      title={profile.displayName}
      subtitle={`${isDefault ? "默认 · " : ""}${profile.status?.hasApiKey ? "已配置" : "未配置"}`}
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
            onRevealError={(error) => setMessage(`读取 API Key 失败：${String(error)}`)}
          />
        </Field>
        <Field label="模型" hint="可填写该供应商支持的任意模型名称">
          <Input value={model} onChange={(event) => setModel(event.target.value)} />
        </Field>
        {isCustom && (
          <Field
            className="sm:col-span-2"
            label="接口地址"
            hint="填写 OpenAI 兼容接口的基础地址，例如 https://example.com/v1/"
          >
            <Input
              value={endpoint}
              placeholder="https://example.com/v1/"
              onChange={(event) => setEndpoint(event.target.value)}
            />
          </Field>
        )}
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
        <Button size="sm" variant="primary" onClick={save}>保存</Button>
        {!isDefault && <Button size="sm" onClick={() => void setDefault("llm", profile.id)}>设为默认</Button>}
        {!isBuiltin && <Button size="sm" variant="danger" onClick={deleteProfile}>删除</Button>}
      </div>
      {message && <p className="mt-2 text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </Collapse>
  );
}

export function SettingsLlmPanel() {
  const profiles = useProviderStore((state) => state.profiles).filter((profile) => profile.kind.startsWith("llm:"));
  const defaults = useProviderStore((state) => state.defaults);
  const setDefault = useProviderStore((state) => state.setDefault);
  const add = useProviderStore((state) => state.addLlmProvider);
  const [open, setOpen] = useState(false);
  const [adapter, setAdapter] = useState("groq");
  const [displayName, setDisplayName] = useState("Groq");
  const [model, setModel] = useState("openai/gpt-oss-20b");
  const [apiKey, setApiKey] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [message, setMessage] = useState("");

  const selectPreset = (nextAdapter: string) => {
    const preset = PRESETS.find((item) => item.adapter === nextAdapter) ?? PRESETS[0];
    setAdapter(preset.adapter);
    setDisplayName(preset.name);
    setModel(preset.model);
    setEndpoint("");
  };

  const submit = async () => {
    try {
      await add({ adapter, displayName, model, apiKey, endpoint });
      setOpen(false);
      setApiKey("");
      setMessage("");
    } catch (error) {
      setMessage(`添加失败：${String(error)}`);
    }
  };

  return (
    <SettingsSection title="大语言模型">
      <Field
        label="默认模型供应商"
        controlId="default-llm-provider"
        hint="智能文本处理会使用这里选中的供应商"
        actions={<Button variant="primary" onClick={() => setOpen(true)}>+ 添加</Button>}
      >
          <Select id="default-llm-provider" value={defaults.llm} onChange={(event) => void setDefault("llm", event.target.value)}>
            {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.displayName}</option>)}
          </Select>
      </Field>

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
          <Field label="模型">
            <Input value={model} placeholder="模型名称" onChange={(event) => setModel(event.target.value)} />
          </Field>
          {adapter === "custom" && (
            <Field label="接口地址" hint="OpenAI 兼容接口的基础地址">
              <Input value={endpoint} placeholder="https://example.com/v1/" onChange={(event) => setEndpoint(event.target.value)} />
            </Field>
          )}
          <Field label="API Key" hint="也可以添加后再填写">
            <Input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} />
          </Field>
          {message && <p className="text-xs text-[var(--color-err)]">{message}</p>}
          <div className="flex justify-end gap-2">
            <Button onClick={() => setOpen(false)}>取消</Button>
            <Button variant="primary" onClick={submit}>添加</Button>
          </div>
        </div>
      </Modal>
    </SettingsSection>
  );
}
