import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { SettingsProviderPanel } from "@/views/SettingsProviderPanel";
import { SettingsLlmPanel } from "@/views/SettingsLlmPanel";
import { PluginManagerPanel } from "@/views/PluginManagerPanel";
import { SettingsStartupPanel } from "@/views/SettingsStartupPanel";
import { SettingsMicCuePanel } from "@/views/SettingsMicCuePanel";
import { SettingsAppearancePanel } from "@/views/SettingsAppearancePanel";
import { SettingsComparePanel } from "@/views/SettingsComparePanel";
import { SettingsAdvancedPanel } from "@/views/SettingsAdvancedPanel";
import { useUiStore, type SettingsTabKey } from "@/store/useUiStore";

const TABS: TabItem<SettingsTabKey>[] = [
  { key: "model", label: "模型" },
  { key: "plugins", label: "插件" },
  { key: "audio", label: "音频" },
  { key: "general", label: "通用" },
  { key: "compare", label: "对比" },
  { key: "advanced", label: "高级" },
];

export function SettingsView() {
  const tab = useUiStore((state) => state.settingsTab);
  const setTab = useUiStore((state) => state.setSettingsTab);

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="设置"
        description="配置识别模型与密钥、插件、麦克风与提示音、启动与外观，并支持多模型效果对比与音频链路调校。"
      />

      <Tabs<SettingsTabKey>
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
        {tab === "model" && (
          <div className="flex flex-col gap-7">
            <SettingsProviderPanel />
            <SettingsLlmPanel />
          </div>
        )}
        {tab === "plugins" && <PluginManagerPanel />}
        {tab === "audio" && <SettingsMicCuePanel />}
        {tab === "general" && (
          <div className="flex flex-col gap-7">
            <SettingsStartupPanel />
            <SettingsAppearancePanel />
          </div>
        )}
        {tab === "compare" && <SettingsComparePanel />}
        {tab === "advanced" && <SettingsAdvancedPanel />}
      </div>
    </div>
  );
}
