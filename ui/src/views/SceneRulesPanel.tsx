import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { AppProfilesPanel } from "@/views/AppProfilesPanel";
import { ShortcutProfilesPanel } from "@/views/ShortcutProfilesPanel";
import { useUiStore, type SceneRulesTabKey } from "@/store/useUiStore";

const TABS: TabItem<SceneRulesTabKey>[] = [
  { key: "apps", label: "按软件" },
  { key: "shortcuts", label: "按快捷键" },
];

export function SceneRulesPanel() {
  const tab = useUiStore((state) => state.sceneRulesTab);
  const setTab = useUiStore((state) => state.setSceneRulesTab);
  return (
    <div className="flex flex-col gap-6">
      <Tabs<SceneRulesTabKey>
        id="scene-rule-tabs"
        ariaLabel="场景规则类型"
        variant="subpage"
        className="-mt-4"
        tabs={TABS}
        active={tab}
        onChange={setTab}
      />
      <div
        id={`scene-rule-tabs-${tab}-panel`}
        role="tabpanel"
        aria-labelledby={`scene-rule-tabs-${tab}-tab`}
      >
        {tab === "apps" ? <AppProfilesPanel /> : <ShortcutProfilesPanel />}
      </div>
    </div>
  );
}
