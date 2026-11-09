import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { SecretInput } from "@/components/ui/SecretInput";
import { recommendedLlmModels } from "@/features/models/recommendedLlmModels";
import { ApiKeyLink, API_KEY_URLS_BY_ADAPTER } from "@/features/settings/apiKeyLinks";
import { useProviderStore } from "@/store/useProviderStore";

/**
 * 可添加的大模型服务。阿里云百炼、火山引擎（方舟）与 Groq 已内置且包含大模型，不在这里出现。
 * 有推荐模型目录的供应商以目录第一项为默认模型，没有目录的才在这里写默认模型。
 */
const PRESETS = [
  { adapter: "openai", name: "OpenAI", model: "gpt-4o-mini", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.openai },
  { adapter: "anthropic", name: "Anthropic", model: "claude-haiku-4-5", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.anthropic },
  { adapter: "gemini", name: "Google Gemini", model: "gemini-2.5-flash", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.gemini },
  { adapter: "kimi", name: "Kimi", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.kimi },
  { adapter: "bigmodel", name: "智谱 GLM", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.bigmodel },
  { adapter: "deepseek", name: "DeepSeek", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.deepseek },
  { adapter: "mimo", name: "小米 MiMo", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.mimo },
  { adapter: "minimax", name: "MiniMax", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.minimax },
  { adapter: "open_router", name: "OpenRouter", apiKeyUrl: API_KEY_URLS_BY_ADAPTER.open_router },
  { adapter: "custom", name: "自定义 OpenAI 兼容接口", apiKeyUrl: null },
] as const;

export function AddProviderModal({ open, onClose, onAdded }: {
  open: boolean;
  onClose: () => void;
  onAdded: (providerId: string) => void;
}) {
  const add = useProviderStore((state) => state.addLlmProvider);
  const [adapter, setAdapter] = useState<string>(PRESETS[0].adapter);
  const [apiKey, setApiKey] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [error, setError] = useState("");
  const [adding, setAdding] = useState(false);
  const preset = PRESETS.find((item) => item.adapter === adapter) ?? PRESETS[0];
  const endpointValid = adapter !== "custom" || /^https?:\/\//.test(endpoint.trim());

  const close = () => {
    setApiKey("");
    setEndpoint("");
    setError("");
    onClose();
  };

  const submit = async () => {
    setAdding(true);
    try {
      const model = "model" in preset ? preset.model : (recommendedLlmModels(`llm:${adapter}`)[0]?.id ?? "");
      const profile = await add({ adapter, displayName: preset.name, model, apiKey: apiKey.trim(), endpoint });
      close();
      onAdded(profile.id);
    } catch (reason) {
      setError(`添加失败：${String(reason)}`);
    } finally {
      setAdding(false);
    }
  };

  return (
    <Modal open={open} onClose={close} title="添加大模型服务" ariaLabel="添加大模型服务" className="max-w-xl">
      <div className="flex flex-col gap-4 p-5">
        <Field label="服务">
          <Select value={adapter} onChange={(event) => { setAdapter(event.target.value); setEndpoint(""); setError(""); }}>
            {PRESETS.map((item) => <option key={item.adapter} value={item.adapter}>{item.name}</option>)}
          </Select>
        </Field>
        {adapter === "custom" && (
          <Field label="接口地址" hint="填写服务商提供的服务地址（需支持 OpenAI 格式）">
            <Input value={endpoint} placeholder="https://example.com/v1/" onChange={(event) => setEndpoint(event.target.value)} />
          </Field>
        )}
        <Field
          label="API Key"
          controlId="new-llm-api-key"
          actions={preset.apiKeyUrl && <ApiKeyLink url={preset.apiKeyUrl} label={`获取 ${preset.name} API Key`} onError={(reason) => setError(`打开密钥页面失败：${reason}`)} />}
        >
          <SecretInput
            id="new-llm-api-key"
            required
            autoComplete="new-password"
            draftValue={apiKey}
            hasStoredValue={false}
            onDraftChange={setApiKey}
          />
        </Field>
        {error && <p role="alert" className="text-xs text-[var(--color-err)]">{error}</p>}
        <div className="flex justify-end gap-2">
          <Button onClick={close} disabled={adding}>取消</Button>
          <Button variant="primary" onClick={() => void submit()} disabled={adding || !apiKey.trim() || !endpointValid}>
            {adding ? "正在添加" : "添加"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
