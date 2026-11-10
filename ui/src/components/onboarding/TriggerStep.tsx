import { Group } from "@/components/ui/Group";
import { Textarea } from "@/components/ui/Input";
import { shortcutLabel } from "@/features/dictation/hotkeys";
import { useDictationStore } from "@/store/useDictationStore";
import { OrbGestureSection, ShortcutRows } from "@/views/dictation/DictationSections";
import { StepBody, StepError, StepFooter } from "./StepLayout";

export function TriggerStep({
  onBack,
  onFinish,
  finishing,
  error,
}: {
  onBack: () => void;
  onFinish: () => void;
  finishing: boolean;
  error: string;
}) {
  const shortcut = useDictationStore((state) => state.shortcut);
  const label = shortcutLabel(shortcut);
  return (
    <>
      <StepBody label="唤起方式">
        <Group><ShortcutRows /></Group>
        <OrbGestureSection />
        <Textarea
          aria-label="试一试"
          rows={3}
          placeholder={label ? `按一下 ${label}，说句话试试…` : "设置快捷键后，按一下说句话试试…"}
        />
        {error && <StepError>{error}</StepError>}
      </StepBody>
      <StepFooter onBack={onBack} label={finishing ? "正在保存…" : "开始使用"} onNext={onFinish} disabled={finishing} />
    </>
  );
}
