import { AudioLines, Bot, GitCompareArrows, Puzzle } from "lucide-react";
import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { useSearchHitHighlight } from "@/features/settings/SettingsSearch";
import { useUiStore, type ModelsSectionKey } from "@/store/useUiStore";
import { SettingsProviderPanel } from "@/views/SettingsProviderPanel";
import { SettingsLlmPanel } from "@/views/SettingsLlmPanel";
import { PluginManagerPanel } from "@/views/PluginManagerPanel";
import { SettingsComparePanel } from "@/views/SettingsComparePanel";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const ITEMS: NodeItem<ModelsSectionKey>[] = [
  { key: "asr", label: "识别、翻译与读屏", icon: <AudioLines {...iconProps} /> },
  { key: "llm", label: "大模型", icon: <Bot {...iconProps} /> },
  { key: "plugins", label: "插件与本地模型", icon: <Puzzle {...iconProps} /> },
  { key: "compare", label: "模型对比", icon: <GitCompareArrows {...iconProps} /> },
];

const PANEL_ID = "models-section-panel";

export function ModelsPage() {
  const section = useUiStore((s) => s.modelsSection);
  const setSection = useUiStore((s) => s.setModelsSection);
  useSearchHitHighlight("models", section, PANEL_ID);

  return (
    <MasterDetail title="模型与密钥" list={<NodeList ariaLabel="模型分类" items={ITEMS} active={section} onSelect={setSection} />}>
      <InspectorHeader title={ITEMS.find((item) => item.key === section)?.label} />
      <div id={PANEL_ID}>
        {section === "asr" && <SettingsProviderPanel />}
        {section === "llm" && <SettingsLlmPanel />}
        {section === "plugins" && <PluginManagerPanel />}
        {section === "compare" && <SettingsComparePanel />}
      </div>
    </MasterDetail>
  );
}
