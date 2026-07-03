import { useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { SettingsProviderPanel } from "@/views/SettingsProviderPanel";
import { SettingsStartupPanel } from "@/views/SettingsStartupPanel";
import { SettingsMicCuePanel } from "@/views/SettingsMicCuePanel";
import { AudioView } from "@/views/AudioView";
import { SettingsAppearancePanel } from "@/views/SettingsAppearancePanel";

type TabKey = "provider" | "audio" | "startup" | "mic" | "appearance";

const TABS: TabItem<TabKey>[] = [
  { key: "provider", label: "密钥与识别" },
  { key: "audio", label: "录音调整" },
  { key: "startup", label: "启动设置" },
  { key: "mic", label: "麦克风与提示音" },
  { key: "appearance", label: "外观" },
];

export function SettingsView() {
  const [tab, setTab] = useState<TabKey>("provider");

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="设置"
        description="配置识别密钥、录音处理、启动方式、麦克风与提示音以及界面外观。"
      />

      <Tabs<TabKey> tabs={TABS} active={tab} onChange={setTab} />

      {tab === "provider" && <SettingsProviderPanel />}
      {tab === "audio" && <AudioView />}
      {tab === "startup" && <SettingsStartupPanel />}
      {tab === "mic" && <SettingsMicCuePanel />}
      {tab === "appearance" && <SettingsAppearancePanel />}
    </div>
  );
}
