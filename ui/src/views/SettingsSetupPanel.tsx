import { Button } from "@/components/ui/Button";
import { SettingsSection } from "@/components/ui/SettingsSection";

export function SettingsSetupPanel() {
  return (
    <SettingsSection title="环境体检">
      <p className="text-xs text-[var(--color-fg-subtle)]">重新检查麦克风、系统权限、识别模型、快捷键和文本注入。</p>
      <Button onClick={() => window.dispatchEvent(new Event("sayit-open-setup"))}>运行首次使用体检</Button>
    </SettingsSection>
  );
}
