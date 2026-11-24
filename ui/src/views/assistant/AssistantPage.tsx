import { useState } from "react";
import { Languages, MessageCircleQuestion, PencilLine } from "lucide-react";
import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { FeatureDemo, type FeatureDemoKind } from "@/components/FeatureDemo";
import { shortcutLabel } from "@/features/dictation/hotkeys";
import { shortcutBindingsQuery } from "@/features/hotkeys/catalog";
import { useCachedQuery } from "@/hooks/useCachedQuery";
import { useUiStore, type AssistantActionKey } from "@/store/useUiStore";
import { AssistantFeaturePanel } from "@/views/VoiceAssistantPanel";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const ITEMS: NodeItem<AssistantActionKey>[] = [
  { key: "translateSpeech", label: "翻译输入", icon: <Languages {...iconProps} /> },
  { key: "editSelection", label: "选区编辑", icon: <PencilLine {...iconProps} /> },
  { key: "ask", label: "语音问答", icon: <MessageCircleQuestion {...iconProps} /> },
];

const DEMO_KIND: Record<AssistantActionKey, FeatureDemoKind> = {
  translateSpeech: "translate",
  editSelection: "edit",
  ask: "ask",
};

export function AssistantPage() {
  const action = useUiStore((s) => s.assistantAction);
  const setAction = useUiStore((s) => s.setAssistantAction);
  // 鼠标停在中栏某一条上时，示意换成那一条并播放。
  const [hovered, setHovered] = useState<AssistantActionKey | null>(null);
  const bindings = useCachedQuery(shortcutBindingsQuery);
  const shown = hovered ?? action;
  const binding = bindings.data?.find((entry) => entry.target.kind === "assistant" && entry.target.action === shown);
  const demo = (
    <FeatureDemo
      kind={DEMO_KIND[shown]}
      play={hovered !== null}
      keyLabel={(binding && shortcutLabel(binding)) || undefined}
      hold={binding?.triggerMode === "pressHold"}
    />
  );
  return (
    <MasterDetail title="助手" list={<NodeList<AssistantActionKey> ariaLabel="助手动作" items={ITEMS} active={action} onSelect={setAction} onHover={setHovered} />}>
      <InspectorHeader title={ITEMS.find((item) => item.key === action)?.label} />
      <AssistantFeaturePanel key={action} action={action} demo={demo} />
    </MasterDetail>
  );
}
