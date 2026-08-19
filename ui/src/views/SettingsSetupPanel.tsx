import { Button } from "@/components/ui/Button";
import { SettingsSection } from "@/components/ui/SettingsSection";

export function SettingsSetupPanel() {
  return (
    <SettingsSection title="使用引导">
      <p className="text-xs text-[var(--color-fg-subtle)]">重新检查权限、选择主识别模型，或查看离线模型的下载与安装方式。</p>
      <Button onClick={() => window.dispatchEvent(new Event("sayit-open-setup"))}>重新运行使用引导</Button>
    </SettingsSection>
  );
}
