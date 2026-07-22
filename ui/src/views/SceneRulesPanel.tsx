import { useState } from "react";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { AppProfilesPanel } from "@/views/AppProfilesPanel";
import { ShortcutProfilesPanel } from "@/views/ShortcutProfilesPanel";

type SceneRuleTab = "apps" | "shortcuts";

const TABS: TabItem<SceneRuleTab>[] = [
  { key: "apps", label: "按软件" },
  { key: "shortcuts", label: "按快捷键" },
];

export function SceneRulesPanel() {
  const [tab, setTab] = useState<SceneRuleTab>("apps");
  return (
    <div className="flex flex-col gap-6">
      <Tabs<SceneRuleTab>
        id="scene-rule-tabs"
        ariaLabel="场景规则类型"
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
