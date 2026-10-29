import { Select } from "@/components/ui/Input";
import { Group } from "@/components/ui/Group";
import { Switch } from "@/components/ui/Switch";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useUiStore, type DictationNodeKey } from "@/store/useUiStore";
import { LocalRulesPanel } from "@/views/LocalRulesPanel";
import { SmartTextPanel } from "@/views/SmartTextPanel";
import { AppProfilesPanel } from "@/views/AppProfilesPanel";
import { ShortcutProfilesPanel } from "@/views/ShortcutProfilesPanel";
import { useState } from "react";
import { shortcutLabel } from "@/features/dictation/hotkeys";
import type { DictPrefs } from "@/store/useDictPrefs";
import { AsrSection, InjectMethodRow, MicRow, TriggerSection } from "./DictationSections";

type SceneKey = "default" | "apps" | "shortcuts";

const NODE_TITLES: Record<DictationNodeKey, string> = {
  trigger: "触发",
  audio: "声音",
  asr: "识别",
  smart: "智能处理",
  rules: "本地规则",
  output: "输出",
  apps: "按软件区分",
  shortcuts: "按快捷键区分",
};

export function DictationPage() {
  useModelCatalogRevision();
  const node = useUiStore((s) => s.dictationNode);
  const setNode = useUiStore((s) => s.setDictationNode);
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const { shortcut, pressHoldMode, injectMethod } = useDictationStore();
  const [saveError, setSaveError] = useState("");
  const toggle = (changes: Partial<DictPrefs>) => {
    setSaveError("");
    void patch(changes).catch((error) => setSaveError(`保存失败：${String(error)}`));
  };
  const errorLine = saveError && <p role="alert" className="-mt-2 mb-4 text-xs text-[var(--color-err)]">{saveError}</p>;

  const scene: SceneKey = node === "apps" || node === "shortcuts" ? node : "default";
  const asrLabel = DICTATION_ASR_MODEL_OPTIONS.find((item) => item.value === prefs.asrModel)?.label ?? prefs.asrModel;
  const activeTemplate = prefs.smartTemplates.find((item) => item.id === prefs.smartTemplateId)?.name ?? "";
  const enabledRules = prefs.localRules.filter((rule) => rule.enabled).length;

  const items: NodeItem<DictationNodeKey>[] = [
    { key: "trigger", label: "触发", value: `${shortcutLabel(shortcut) || "未设置"} · ${pressHoldMode ? "按住" : "按一下"}` },
    { key: "audio", label: "声音", value: prefs.micDeviceId || "系统默认麦克风" },
    { key: "asr", label: "识别", value: asrLabel },
    { key: "smart", label: "智能处理", value: prefs.smartProcessingEnabled ? activeTemplate : "未开启", state: prefs.smartProcessingEnabled ? "on" : "off" },
    { key: "rules", label: "本地规则", value: prefs.localRulesEnabled ? `${enabledRules} 条` : "未开启", state: prefs.localRulesEnabled ? undefined : "off" },
    { key: "output", label: "输出", value: injectMethod === "paste" ? "粘贴到光标" : "逐字输入" },
  ];

  const sceneSelect = (
    <Select
      size="sm"
      className="w-[7.5rem]"
      aria-label="场景"
      value={scene}
      onChange={(event) => {
        const next = event.target.value as SceneKey;
        setNode(next === "default" ? "trigger" : next);
      }}
    >
      <option value="default">默认场景</option>
      <option value="apps">按软件区分</option>
      <option value="shortcuts">按快捷键区分</option>
    </Select>
  );

  return (
    <MasterDetail
      title="听写"
      titleRight={sceneSelect}
      list={scene === "default"
        ? <NodeList chain ariaLabel="听写流程" items={items} active={node} onSelect={setNode} />
        : <p className="px-1.5 text-[13px] leading-relaxed text-[var(--color-fg-subtle)]">{scene === "apps" ? "在指定软件里听写时，改用不同的处理方式。" : "用另一个快捷键，套用不同的处理方式。"}</p>}
    >
      {node === "trigger" && <><InspectorHeader title={NODE_TITLES.trigger} /><InspectorBody><TriggerSection /></InspectorBody></>}
      {node === "audio" && <><InspectorHeader title={NODE_TITLES.audio} /><InspectorBody><Group><MicRow /></Group></InspectorBody></>}
      {node === "asr" && <><InspectorHeader title={NODE_TITLES.asr} /><InspectorBody><AsrSection /></InspectorBody></>}
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
      {node === "output" && <><InspectorHeader title={NODE_TITLES.output} /><InspectorBody><Group><InjectMethodRow /></Group></InspectorBody></>}
      {node === "apps" && <><InspectorHeader title={NODE_TITLES.apps} /><AppProfilesPanel /></>}
      {node === "shortcuts" && <><InspectorHeader title={NODE_TITLES.shortcuts} /><ShortcutProfilesPanel /></>}
    </MasterDetail>
  );
}
