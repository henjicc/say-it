import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { SecretInput } from "@/components/ui/SecretInput";
import { providerReady } from "@/features/models/providerModels";
import { useLocalModelProgress } from "@/features/models/localModels";
import {
  ASR_CHOICES,
  defaultAsrChoice,
  apiKeyLabelFor,
  apiKeyUrlFor,
  saveProviderKey,
  type AsrChoiceKey,
  type UsageMode,
} from "@/features/onboarding/setup";
import type { HardwareSummary } from "@/features/models/localModelNeeds";
import { ApiKeyLink } from "@/features/settings/apiKeyLinks";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { ChoiceList } from "./ChoiceList";
import { LocalPackDownload } from "./LocalPackDownload";
import { StepBody, StepError, StepFooter } from "./StepLayout";

export function AsrStep({
  mode,
  hardware,
  choice,
  onChoice,
  onBack,
  onNext,
}: {
  mode: UsageMode;
  hardware: HardwareSummary | undefined;
  choice: AsrChoiceKey;
  onChoice: (key: AsrChoiceKey) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const profiles = useProviderStore((state) => state.profiles);
  const loadProviders = useProviderStore((state) => state.load);
  const providersError = useProviderStore((state) => (state.statusTone === "err" ? state.statusText : ""));
  const patchDictPrefs = useDictPrefs((state) => state.patch);
  const [drafts, setDrafts] = useState<Partial<Record<AsrChoiceKey, string>>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const selected = ASR_CHOICES.find((item) => item.key === choice)!;
  const packs = "packs" in selected ? selected.packs : undefined;
  const local = useLocalModelProgress(packs ?? []);
  const provider = "providerId" in selected ? profiles.find((profile) => profile.id === selected.providerId) : undefined;
  const draft = drafts[choice]?.trim() ?? "";
  const keyReady = Boolean(provider && providerReady(provider, "asr"));
  const ready = packs ? local.ready : Boolean(provider) && (keyReady || Boolean(draft));

  const recommended = defaultAsrChoice(mode, hardware);
  const items = ASR_CHOICES.filter((item) => item.mode === mode).map((item) => ({
    key: item.key,
    title: item.label,
    description: item.description,
    recommended: item.key === recommended,
  }));

  const next = async () => {
    setSaving(true);
    setError("");
    try {
      if (provider && draft) await saveProviderKey(provider, "asr", draft);
      await patchDictPrefs({ asrModel: selected.model });
      onNext();
    } catch (reason) {
      setError(`保存失败：${String(reason)}`);
    } finally {
      setSaving(false);
    }
  };

  const detail = (key: AsrChoiceKey) => {
    const item = ASR_CHOICES.find((entry) => entry.key === key)!;
    if ("packs" in item) return <LocalPackDownload ids={item.packs} />;
    if (!provider) {
      return providersError ? (
        <div className="flex flex-col items-start gap-2">
          <StepError>{providersError}</StepError>
          <Button size="sm" onClick={() => void loadProviders()}>重试</Button>
        </div>
      ) : <p role="status" className="text-xs text-[var(--color-fg-subtle)]">正在读取供应商…</p>;
    }
    const label = apiKeyLabelFor(provider, "asr");
    const keyUrl = apiKeyUrlFor(provider, "asr");
    return (
      <div className="flex flex-col gap-2">
        <SecretInput
          aria-label={label}
          draftValue={drafts[key] ?? ""}
          hasStoredValue={keyReady}
          placeholder={keyReady ? "已保存，输入新值可覆盖" : `粘贴 ${label}`}
          onDraftChange={(value) => setDrafts((current) => ({ ...current, [key]: value }))}
        />
        {keyUrl && (
          <span className="text-xs">
            <ApiKeyLink url={keyUrl} label="获取密钥" ariaLabel={`获取 ${label}`} onError={(reason) => setError(`打开密钥页面失败：${reason}`)} />
          </span>
        )}
      </div>
    );
  };

  return (
    <>
      <StepBody label="识别模型">
        <ChoiceList label="语音识别方式" items={items} value={choice} onChange={onChoice} detail={detail} />
        <p className="text-xs text-[var(--color-fg-subtle)]">其他模型和供应商：之后在「模型」页添加</p>
        {error && <StepError>{error}</StepError>}
      </StepBody>
      <StepFooter
        onBack={onBack}
        label="下一步"
        onNext={() => void next()}
        disabled={!ready || saving}
        reason={ready ? undefined : packs ? (local.downloading ? "下载完成后继续" : "下载模型后继续") : "填好密钥后继续"}
      />
    </>
  );
}
