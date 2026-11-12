import { Hash, TextQuote } from "lucide-react";
import { MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { useUiStore, type LexiconSectionKey } from "@/store/useUiStore";
import { ContextTab, HotwordsTab } from "@/views/CustomizationView";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const ITEMS: NodeItem<LexiconSectionKey>[] = [
  { key: "hotwords", label: "热词", icon: <Hash {...iconProps} /> },
  { key: "context", label: "上下文", icon: <TextQuote {...iconProps} /> },
];

export function LexiconPage() {
  const section = useUiStore((s) => s.lexiconSection);
  const setSection = useUiStore((s) => s.setLexiconSection);

  return (
    <MasterDetail title="词库" list={<NodeList ariaLabel="词库分类" items={ITEMS} active={section} onSelect={setSection} />}>
      {section === "hotwords" ? <HotwordsTab /> : <ContextTab />}
    </MasterDetail>
  );
}
