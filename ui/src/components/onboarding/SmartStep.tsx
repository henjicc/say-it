import { useState } from "react";
import { CheckCircle2 } from "lucide-react";
import { Select } from "@/components/ui/Input";
import { SecretInput } from "@/components/ui/SecretInput";
import { LOCAL_MODEL_NEEDS, type HardwareSummary } from "@/features/models/localModelNeeds";
import { useLocalModelProgress } from "@/features/onboarding/localModels";
import {
  LLM_TARGETS,
  LOCAL_POLISH_PACK_ID,
  applySmartChoice,
  localPolishFits,
  llmTargetDefaultModel,
  llmTargetKeyLabel,
  llmTargetKeyUrl,
  llmTargetLabel,
  llmTargetModels,
  llmTargetReady,
  type AsrChoiceKey,
  type LlmTargetKey,
  type SmartChoice,
  type UsageMode,
} from "@/features/onboarding/setup";
import { ApiKeyLink } from "@/features/settings/apiKeyLinks";
import { useProviderStore } from "@/store/useProviderStore";
import { ChoiceList, type ChoiceItem } from "./ChoiceList";
import { LocalPackDownload } from "./LocalPackDownload";
import { StepBody, StepError, StepFooter } from "./StepLayout";

const RULES = { key: "rules", title: "只用本地规则", description: "去掉嗯、呃等语气词，不联网、不收费" } as const;
const NONE = { key: "none", title: "不整理", description: "识别出什么就输入什么" } as const;

export function SmartStep({
  mode,
  hardware,
  asrChoice,
  onBack,
  onNext,
}: {
  mode: UsageMode;
  hardware: HardwareSummary | undefined;
  asrChoice: AsrChoiceKey;
  onBack: () => void;
  onNext: () => void;
}) {
  const profiles = useProviderStore((state) => state.profiles);
  const polishFits = localPolishFits(hardware);
  const choices: readonly ChoiceItem<SmartChoice>[] = mode === "local"
    ? [
      {
        key: "local",
        title: "本地整理 · Qwen3.5-4B 听写微调",
        description: `去掉口头禅、理顺句子、补全标点，不联网；${LOCAL_MODEL_NEEDS[LOCAL_POLISH_PACK_ID].summary}`,
        recommended: polishFits,
      },
      { ...RULES, recommended: !polishFits },
      NONE,
    ]
    : [
      { key: "smart", title: "智能整理", description: "用大模型去掉口头禅、理顺句子、补全标点", recommended: true },
      RULES,
      NONE,
    ];
  const [choice, setChoice] = useState<SmartChoice>(mode === "online" ? "smart" : polishFits ? "local" : "rules");
  const polish = useLocalModelProgress([LOCAL_POLISH_PACK_ID]);
  // 识别用的是百炼时它的密钥也能调大模型，直接推荐；其他情况推荐 DeepSeek。
  const [targetKey, setTargetKey] = useState<LlmTargetKey>(asrChoice === "bailian" ? "bailian" : "deepseek");
  const target = LLM_TARGETS.find((item) => item.key === targetKey)!;
  const models = llmTargetModels(target, profiles);
  const [model, setModel] = useState(() => llmTargetDefaultModel(target, profiles));
  const [drafts, setDrafts] = useState<Partial<Record<LlmTargetKey, string>>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const draft = drafts[targetKey]?.trim() ?? "";
  const keyReady = llmTargetReady(target, profiles);
  const reusedBailianKey = targetKey === "bailian" && asrChoice === "bailian" && keyReady;
  const ready = choice === "local" ? polish.ready : choice !== "smart" || keyReady || Boolean(draft);
  const keyUrl = llmTargetKeyUrl(target, profiles);
  const keyLabel = llmTargetKeyLabel(target, profiles);

  const changeTarget = (key: LlmTargetKey) => {
    setTargetKey(key);
    setModel(llmTargetDefaultModel(LLM_TARGETS.find((item) => item.key === key)!, profiles));
  };

  const next = async () => {
    setSaving(true);
    setError("");
    try {
      await applySmartChoice(choice, { target, model, apiKey: draft });
      onNext();
    } catch (reason) {
      setError(`保存失败：${String(reason)}`);
    } finally {
      setSaving(false);
    }
  };

  const detail = (key: SmartChoice) => key === "local" ? <LocalPackDownload ids={[LOCAL_POLISH_PACK_ID]} /> : key === "smart" && (
    <div className="flex flex-col gap-2">
      <div className="flex gap-2">
        <Select className="min-w-0 flex-1" aria-label="大模型服务" value={targetKey} onChange={(event) => changeTarget(event.target.value as LlmTargetKey)}>
          {LLM_TARGETS.map((item) => <option key={item.key} value={item.key}>{llmTargetLabel(item, profiles)}</option>)}
        </Select>
        <Select className="min-w-0 flex-1" aria-label="大模型" value={model} onChange={(event) => setModel(event.target.value)}>
          {models.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
        </Select>
      </div>
      {reusedBailianKey ? (
        <p className="flex items-center gap-1.5 text-xs text-[var(--color-ok)]"><CheckCircle2 className="h-3.5 w-3.5" aria-hidden />沿用识别模型的百炼密钥</p>
      ) : (
        <>
          <SecretInput
            aria-label={keyLabel}
            draftValue={drafts[targetKey] ?? ""}
            hasStoredValue={keyReady}
            placeholder={keyReady ? "已保存，输入新值可覆盖" : `粘贴 ${keyLabel}`}
            onDraftChange={(value) => setDrafts((current) => ({ ...current, [targetKey]: value }))}
          />
          {keyUrl && (
            <span className="text-xs">
              <ApiKeyLink url={keyUrl} label="获取密钥" ariaLabel={`获取 ${keyLabel}`} onError={(reason) => setError(`打开密钥页面失败：${reason}`)} />
            </span>
          )}
        </>
      )}
    </div>
  );

  return (
    <>
      <StepBody label="智能整理">
        <ChoiceList<SmartChoice> label="识别后的文字怎么整理" items={choices} value={choice} onChange={setChoice} detail={detail} />
        {error && <StepError>{error}</StepError>}
      </StepBody>
      <StepFooter
        onBack={onBack}
        label="下一步"
        onNext={() => void next()}
        disabled={!ready || saving}
        reason={ready ? undefined : choice === "local" ? (polish.downloading ? "下载完成后继续" : "下载模型后继续，或选择不用模型") : "填好密钥后继续，或选择不用大模型"}
      />
    </>
  );
}
