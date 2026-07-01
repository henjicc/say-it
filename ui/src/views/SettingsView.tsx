import { useState } from "react";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { SettingsProviderPanel } from "@/views/SettingsProviderPanel";
import { SettingsStartupPanel } from "@/views/SettingsStartupPanel";
import { SettingsMicCuePanel } from "@/views/SettingsMicCuePanel";
import { AudioView } from "@/views/AudioView";

type TabKey = "provider" | "audio" | "startup" | "mic";

const TABS: TabItem<TabKey>[] = [
  { key: "provider", label: "密钥与识别" },
  { key: "audio", label: "音频调教" },
  { key: "startup", label: "启动设置" },
  { key: "mic", label: "麦克风与提示音" },
];

export function SettingsView() {
  const [tab, setTab] = useState<TabKey>("provider");

  return (
    <div className="flex flex-col gap-4 py-2">
      <Tabs<TabKey> tabs={TABS} active={tab} onChange={setTab} />
      {tab === "provider" && <SettingsProviderPanel />}
      {tab === "audio" && <AudioView />}
      {tab === "startup" && <SettingsStartupPanel />}
      {tab === "mic" && <SettingsMicCuePanel />}
    </div>
  );
}
