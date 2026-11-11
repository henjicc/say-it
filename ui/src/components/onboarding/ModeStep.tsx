import { hardwareLabel, type HardwareSummary } from "@/features/models/localModelNeeds";
import { localPolishFits, type UsageMode } from "@/features/onboarding/setup";
import { ChoiceList } from "./ChoiceList";
import { StepBody, StepFooter } from "./StepLayout";

export function ModeStep({
  mode,
  onMode,
  hardware,
  onBack,
  onNext,
}: {
  mode: UsageMode;
  onMode: (mode: UsageMode) => void;
  hardware: HardwareSummary | undefined;
  onBack: () => void;
  onNext: () => void;
}) {
  const local = localPolishFits(hardware);
  const label = hardwareLabel(hardware);
  const items = [
    {
      key: "local",
      title: "本地优先",
      description: "完全免费、不联网也能用；要下载模型，对电脑性能有要求",
      recommended: local,
    },
    {
      key: "online",
      title: "在线使用",
      description: "效果更好、对电脑没有要求；要申请密钥，按用量付费",
      recommended: !local,
    },
  ] as const;

  return (
    <>
      <StepBody label="使用方式">
        <ChoiceList<UsageMode> label="使用方式" items={items} value={mode} onChange={onMode} />
        {label && (
          <p className="text-xs text-[var(--color-fg-subtle)]">
            这台电脑：{label}。{local ? "跑本地模型没有问题。" : "本地识别可以用，本地整理建议有 6 GB 以上显存的独立显卡。"}
          </p>
        )}
      </StepBody>
      <StepFooter onBack={onBack} label="下一步" onNext={onNext} />
    </>
  );
}
