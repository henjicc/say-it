import { useEffect, useState } from "react";
import { CheckCircle2, Download } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Meter } from "@/components/ui/Meter";
import { SecretInput } from "@/components/ui/SecretInput";
import { providerReady } from "@/features/models/providerModels";
import { useLocalModelProgress, useLocalModels } from "@/features/onboarding/localModels";
import {
  ASR_CHOICES,
  apiKeyLabelFor,
  apiKeyUrlFor,
  saveProviderKey,
  type AsrChoiceKey,
} from "@/features/onboarding/setup";
import { ApiKeyLink } from "@/features/settings/apiKeyLinks";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { ChoiceList } from "./ChoiceList";
import { StepBody, StepError, StepFooter } from "./StepLayout";

const formatMb = (bytes: number) => `${Math.round(bytes / 1024 ** 2)} MB`;

function LocalModelDetail() {
  const { install, installError, loadError } = useLocalModels();
  const { ready, downloading, total, done } = useLocalModelProgress();
  if (ready) {
    return <p className="flex items-center gap-1.5 text-xs text-[var(--color-ok)]"><CheckCircle2 className="h-3.5 w-3.5" aria-hidden />模型已下载</p>;
  }
  if (downloading) {
    return (
      <div className="flex items-center gap-3">
        <Meter value={total ? done / total : 0} label="模型下载进度" className="flex-1" />
        {total > 0 && <span className="shrink-0 text-xs tabular-nums text-[var(--color-fg-subtle)]">{formatMb(done)} / {formatMb(total)}</span>}
      </div>
    );
  }
  return (
    <div className="flex flex-col items-start gap-2">
      <Button size="sm" onClick={() => void install()}>
        <Download className="h-3.5 w-3.5" aria-hidden />{installError ? "重新下载" : "下载模型"}
      </Button>
      {(installError || loadError) && <StepError>{installError || loadError}</StepError>}
    </div>
  );
}

export function AsrStep({
  choice,
  onChoice,
  onBack,
  onNext,
}: {
  choice: AsrChoiceKey;
  onChoice: (key: AsrChoiceKey) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const profiles = useProviderStore((state) => state.profiles);
  const loadProviders = useProviderStore((state) => state.load);
  const providersError = useProviderStore((state) => (state.statusTone === "err" ? state.statusText : ""));
  const patchDictPrefs = useDictPrefs((state) => state.patch);
  const loadLocal = useLocalModels((state) => state.load);
  const local = useLocalModelProgress();
  const [drafts, setDrafts] = useState<Partial<Record<AsrChoiceKey, string>>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    void loadLocal();
  }, [loadLocal]);

  const selected = ASR_CHOICES.find((item) => item.key === choice)!;
  const provider = "providerId" in selected ? profiles.find((profile) => profile.id === selected.providerId) : undefined;
  const draft = drafts[choice]?.trim() ?? "";
  const keyReady = Boolean(provider && providerReady(provider, "asr"));
  const ready = choice === "local" ? local.ready : Boolean(provider) && (keyReady || Boolean(draft));

  const items = ASR_CHOICES.map((item) => ({
    key: item.key,
    title: item.label,
    description: item.key === "local" && local.total ? `${item.description} · 约 ${formatMb(local.total)}（含标点模型）` : item.description,
    recommended: "recommended" in item && item.recommended,
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
    if (key === "local") return <LocalModelDetail />;
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
        reason={ready ? undefined : choice === "local" ? (local.downloading ? "下载完成后继续" : "下载模型后继续") : "填好密钥后继续"}
      />
    </>
  );
}
