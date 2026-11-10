import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Input, Select } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { SecretInput } from "@/components/ui/SecretInput";
import { recommendedLlmModels } from "@/features/models/recommendedLlmModels";
import { LLM_PROVIDER_PRESETS as PRESETS } from "@/features/models/llmPresets";
import { ApiKeyLink } from "@/features/settings/apiKeyLinks";
import { useProviderStore } from "@/store/useProviderStore";


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
