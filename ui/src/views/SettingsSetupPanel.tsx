import { Button } from "@/components/ui/Button";
import { SettingsSection } from "@/components/ui/SettingsSection";

export function SettingsSetupPanel() {
  return (
    <SettingsSection title="使用引导">
      <p className="text-xs text-[var(--color-fg-subtle)]">重新确认识别方式、处理后麦克风音量、系统权限、快捷键和文字输入。</p>
      <Button onClick={() => window.dispatchEvent(new Event("sayit-open-setup"))}>重新运行使用引导</Button>
    </SettingsSection>
  );
}
