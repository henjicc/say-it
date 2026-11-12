import { useState } from "react";
import { AppWindow, Keyboard, ListChecks, MousePointer2, SlidersHorizontal, Sparkles } from "lucide-react";
import { Switch } from "@/components/ui/Switch";
import { HelpLabel } from "@/components/ui/Tooltip";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { useDictPrefs, type DictPrefs } from "@/store/useDictPrefs";
import { useSearchHitHighlight } from "@/features/settings/SettingsSearch";
import { useUiStore, type DictationNodeKey } from "@/store/useUiStore";
import { LocalRulesPanel } from "@/views/LocalRulesPanel";
import { SmartTextPanel } from "@/views/SmartTextPanel";
import { AppProfilesPanel } from "@/views/AppProfilesPanel";
import { ShortcutProfilesPanel } from "@/views/ShortcutProfilesPanel";
import { BasicsSection, OrbGestureSection } from "./DictationSections";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const PANEL_ID = "dictation-node-panel";

const NODE_TITLES: Record<DictationNodeKey, string> = {
  basics: "基础",
  smart: "智能处理",
  rules: "本地规则",
  apps: "按软件区分",
  shortcuts: "快捷键方案",
  orbGesture: "悬浮球与手势",
};

export function DictationPage() {
  const node = useUiStore((s) => s.dictationNode);
  const setNode = useUiStore((s) => s.setDictationNode);
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const [saveError, setSaveError] = useState("");
  useSearchHitHighlight("dictation", node, PANEL_ID);
  const toggle = (changes: Partial<DictPrefs>) => {
    setSaveError("");
    void patch(changes).catch((error) => setSaveError(`保存失败：${String(error)}`));
  };
  const errorLine = saveError && <p role="alert" className="-mt-2 mb-4 text-xs text-[var(--color-err)]">{saveError}</p>;

  const items: NodeItem<DictationNodeKey>[] = [
    // 顺序即流程：怎么开始说话 → 说完怎么处理 → 不同软件里怎么区分。
    { key: "basics", label: NODE_TITLES.basics, icon: <SlidersHorizontal {...iconProps} /> },
    { key: "orbGesture", label: NODE_TITLES.orbGesture, icon: <MousePointer2 {...iconProps} /> },
    { key: "shortcuts", label: NODE_TITLES.shortcuts, icon: <Keyboard {...iconProps} /> },
    { key: "smart", label: NODE_TITLES.smart, icon: <Sparkles {...iconProps} />, off: !prefs.smartProcessingEnabled, dividerBefore: true },
    { key: "rules", label: NODE_TITLES.rules, icon: <ListChecks {...iconProps} />, off: !prefs.localRulesEnabled },
    { key: "apps", label: NODE_TITLES.apps, icon: <AppWindow {...iconProps} />, off: !prefs.appProfilesEnabled, dividerBefore: true },
  ];

  return (
    <MasterDetail title="听写" list={<NodeList ariaLabel="听写设置" items={items} active={node} onSelect={setNode} />}>
      {node === "basics" && <><InspectorHeader title={NODE_TITLES.basics} /><InspectorBody><div id={PANEL_ID} className="contents"><BasicsSection /></div></InspectorBody></>}
      {node === "smart" && (
        <>
          <InspectorHeader
            title={NODE_TITLES.smart}
            right={<Switch checked={prefs.smartProcessingEnabled} onChange={(value) => toggle({ smartProcessingEnabled: value })} label="启用智能处理" />}
          />
          {errorLine}
          <SmartTextPanel />
        </>
      )}
      {node === "rules" && (
        <>
          <InspectorHeader
            title={NODE_TITLES.rules}
            right={<Switch checked={prefs.localRulesEnabled} onChange={(value) => toggle({ localRulesEnabled: value })} label="启用本地规则" />}
          />
          {errorLine}
          <LocalRulesPanel />
        </>
      )}
      {node === "apps" && (
        <>
          <InspectorHeader
            title={<HelpLabel content="规则按从上到下的顺序匹配，使用第一条符合的规则；未匹配时使用默认场景。这里只查看软件名和窗口标题。">{NODE_TITLES.apps}</HelpLabel>}
            right={<Switch checked={prefs.appProfilesEnabled} onChange={(value) => toggle({ appProfilesEnabled: value })} label="启用按软件区分" />}
          />
          {errorLine}
          <AppProfilesPanel />
        </>
      )}
      {node === "shortcuts" && <><InspectorHeader title={NODE_TITLES.shortcuts} /><ShortcutProfilesPanel /></>}
      {node === "orbGesture" && <><InspectorHeader title={NODE_TITLES.orbGesture} /><InspectorBody><OrbGestureSection withAppearance /></InspectorBody></>}
    </MasterDetail>
  );
}
