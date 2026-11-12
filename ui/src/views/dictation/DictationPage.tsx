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
  shortcuts: "按快捷键区分",
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
    // 顺序即流程：怎么开始说话 → 说完默认怎么处理 → 哪些情况换一种处理。
    // 后两项都是对默认处理的例外：快捷键方案选「跟随场景」时用按软件区分的规则，否则以快捷键方案为准。
    { key: "basics", label: NODE_TITLES.basics, icon: <SlidersHorizontal {...iconProps} /> },
    { key: "orbGesture", label: NODE_TITLES.orbGesture, icon: <MousePointer2 {...iconProps} /> },
    { key: "smart", label: NODE_TITLES.smart, icon: <Sparkles {...iconProps} />, off: !prefs.smartProcessingEnabled, dividerBefore: true },
    { key: "rules", label: NODE_TITLES.rules, icon: <ListChecks {...iconProps} />, off: !prefs.localRulesEnabled },
    { key: "apps", label: NODE_TITLES.apps, icon: <AppWindow {...iconProps} />, dividerBefore: true },
    { key: "shortcuts", label: NODE_TITLES.shortcuts, icon: <Keyboard {...iconProps} /> },
  ];

  return (
    <MasterDetail title="语音输入" list={<NodeList ariaLabel="语音输入设置" items={items} active={node} onSelect={setNode} />}>
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
          />
          <AppProfilesPanel />
        </>
      )}
      {node === "shortcuts" && (
        <>
          <InspectorHeader title={<HelpLabel content="按下方案的快捷键时，用这里设定的处理方式代替默认处理；一次录音开始后会一直使用当时的方案。同一个按键可以分别设置单击和长按。">{NODE_TITLES.shortcuts}</HelpLabel>} />
          <ShortcutProfilesPanel />
        </>
      )}
      {node === "orbGesture" && <><InspectorHeader title={NODE_TITLES.orbGesture} /><InspectorBody><OrbGestureSection withAppearance /></InspectorBody></>}
    </MasterDetail>
  );
}
