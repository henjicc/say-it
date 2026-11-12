import { useEffect, useState } from "react";
import { Cloud, GitCompareArrows, HardDrive, Plus, Puzzle, SlidersHorizontal } from "lucide-react";
import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { IconButton } from "@/components/ui/IconButton";
import { useSearchHitHighlight } from "@/features/settings/SettingsSearch";
import { isManagedProvider, providerReady } from "@/features/models/providerModels";
import { useProviderStore } from "@/store/useProviderStore";
import { useUiStore, type ModelsSectionKey } from "@/store/useUiStore";
import { LocalModelsPanel, PluginsPanel } from "@/views/PluginManagerPanel";
import { SettingsComparePanel } from "@/views/SettingsComparePanel";
import { AddProviderModal } from "./AddProviderModal";
import { ProviderPanel } from "./ProviderPanel";
import { UsagePanel } from "./UsagePanel";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;
const PANEL_ID = "models-section-panel";

export function ModelsPage() {
  const section = useUiStore((s) => s.modelsSection);
  const setSection = useUiStore((s) => s.setModelsSection);
  const profiles = useProviderStore((s) => s.profiles);
  const loadProviders = useProviderStore((s) => s.load);
  const [adding, setAdding] = useState(false);
  useSearchHitHighlight("models", section, PANEL_ID);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  const providers = profiles.filter(isManagedProvider);
  const items: NodeItem<ModelsSectionKey>[] = [
    { key: "usage", label: "用途", icon: <SlidersHorizontal {...iconProps} /> },
    {
      key: "local",
      label: "本地模型",
      icon: <HardDrive {...iconProps} />,
      headingBefore: {
        label: "模型来源",
        action: (
          <IconButton size="sm" label="添加大模型服务" onClick={() => setAdding(true)}>
            <Plus className="h-3.5 w-3.5" aria-hidden />
          </IconButton>
        ),
      },
    },
    ...providers.map((profile): NodeItem<ModelsSectionKey> => ({
      key: `provider:${profile.id}`,
      label: profile.displayName,
      icon: <Cloud {...iconProps} />,
      status: providerReady(profile) ? { tone: "ok", label: "已配置" } : { tone: "unset", label: "未配置" },
    })),
    { key: "plugins", label: "插件", icon: <Puzzle {...iconProps} />, dividerBefore: true },
    { key: "compare", label: "模型对比", icon: <GitCompareArrows {...iconProps} /> },
  ];
  const provider = section.startsWith("provider:")
    ? providers.find((profile) => `provider:${profile.id}` === section)
    : undefined;
  // 供应商被删除后回到「用途」，不留一个空白检查器。
  const active = section.startsWith("provider:") && !provider ? "usage" : section;

  return (
    <MasterDetail title="模型与密钥" list={<NodeList ariaLabel="模型分类" items={items} active={active} onSelect={setSection} />}>
      <div id={PANEL_ID}>
        {active === "usage" && <><InspectorHeader title="用途" /><UsagePanel /></>}
        {provider && <ProviderPanel key={provider.id} profile={provider} />}
        {active === "local" && <LocalModelsPanel />}
        {active === "plugins" && <PluginsPanel />}
        {active === "compare" && <><InspectorHeader title="模型对比" /><SettingsComparePanel /></>}
      </div>
      <AddProviderModal open={adding} onClose={() => setAdding(false)} onAdded={(id) => setSection(`provider:${id}`)} />
    </MasterDetail>
  );
}
