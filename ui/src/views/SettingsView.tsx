import { useState } from "react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { SettingsProviderPanel } from "@/views/SettingsProviderPanel";
import { SettingsLlmPanel } from "@/views/SettingsLlmPanel";
import { PluginManagerPanel } from "@/views/PluginManagerPanel";
import { SettingsStartupPanel } from "@/views/SettingsStartupPanel";
import { SettingsMicCuePanel } from "@/views/SettingsMicCuePanel";
import { AudioView } from "@/views/AudioView";
import { SettingsAppearancePanel } from "@/views/SettingsAppearancePanel";
import { SettingsComparePanel } from "@/views/SettingsComparePanel";
import { SettingsDisconnectPanel } from "@/views/SettingsDisconnectPanel";

type TabKey = "provider" | "plugins" | "audio" | "disconnect" | "startup" | "mic" | "appearance" | "compare";

const TABS: TabItem<TabKey>[] = [
  { key: "provider", label: "密钥与识别" },
  { key: "plugins", label: "插件管理" },
  { key: "audio", label: "录音调整" },
  { key: "disconnect", label: "断流设置" },
  { key: "startup", label: "启动设置" },
  { key: "mic", label: "麦克风与提示音" },
  { key: "appearance", label: "外观" },
  { key: "compare", label: "对比" },
];

export function SettingsView() {
  const [tab, setTab] = useState<TabKey>("provider");

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="设置"
        description="配置识别密钥、插件、录音调整、启动方式、麦克风与提示音、界面外观，并支持多模型效果对比。"
      />

      <Tabs<TabKey>
        id="settings-tabs"
        ariaLabel="设置分类"
        tabs={TABS}
        active={tab}
        onChange={setTab}
      />

      <div
        id={`settings-tabs-${tab}-panel`}
        role="tabpanel"
        aria-labelledby={`settings-tabs-${tab}-tab`}
      >
        {tab === "provider" && (
          <div className="flex flex-col gap-7">
            <SettingsProviderPanel />
            <SettingsLlmPanel />
          </div>
        )}
        {tab === "plugins" && <PluginManagerPanel />}
        {tab === "audio" && <AudioView />}
        {tab === "disconnect" && <SettingsDisconnectPanel />}
        {tab === "startup" && <SettingsStartupPanel />}
        {tab === "mic" && <SettingsMicCuePanel />}
        {tab === "appearance" && <SettingsAppearancePanel />}
        {tab === "compare" && <SettingsComparePanel />}
      </div>
    </div>
  );
}
