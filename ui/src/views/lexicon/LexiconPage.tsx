import { Hash, TextQuote } from "lucide-react";
import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { useCustomizationStore } from "@/store/useCustomizationStore";
import { useUiStore, type LexiconSectionKey } from "@/store/useUiStore";
import { ContextTab, HotwordsTab } from "@/views/CustomizationView";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

export function LexiconPage() {
  const section = useUiStore((s) => s.lexiconSection);
  const setSection = useUiStore((s) => s.setLexiconSection);
  const prefs = useCustomizationStore((s) => s.prefs);

  const items: NodeItem<LexiconSectionKey>[] = [
    { key: "hotwords", label: "热词", value: prefs.hotwords.length ? `${prefs.hotwords.length} 个` : "还没有", icon: <Hash {...iconProps} /> },
    { key: "context", label: "上下文", value: prefs.contextTemplate.trim() ? "已填写" : "未填写", icon: <TextQuote {...iconProps} /> },
  ];

  return (
    <MasterDetail title="词库" list={<NodeList ariaLabel="词库分类" items={items} active={section} onSelect={setSection} />}>
      <InspectorHeader title={section === "hotwords" ? "热词" : "上下文"} />
      {section === "hotwords" ? <HotwordsTab /> : <ContextTab />}
    </MasterDetail>
  );
}
