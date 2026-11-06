import { Languages, MessageCircleQuestion, PencilLine } from "lucide-react";
import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { useUiStore, type AssistantActionKey } from "@/store/useUiStore";
import { AssistantFeaturePanel } from "@/views/VoiceAssistantPanel";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const ITEMS: NodeItem<AssistantActionKey>[] = [
  { key: "translateSpeech", label: "语音翻译", icon: <Languages {...iconProps} /> },
  { key: "editSelection", label: "选区编辑", icon: <PencilLine {...iconProps} /> },
  { key: "ask", label: "语音问答", icon: <MessageCircleQuestion {...iconProps} /> },
];

export function AssistantPage() {
  const action = useUiStore((s) => s.assistantAction);
  const setAction = useUiStore((s) => s.setAssistantAction);
  return (
    <MasterDetail title="助手" list={<NodeList ariaLabel="助手动作" items={ITEMS} active={action} onSelect={setAction} />}>
      <InspectorHeader title={ITEMS.find((item) => item.key === action)?.label} />
      <AssistantFeaturePanel key={action} action={action} />
    </MasterDetail>
  );
}
