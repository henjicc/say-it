import { FileAudio, SlidersHorizontal, Users } from "lucide-react";
import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { useTranscriptionStore, type TranscriptionTab } from "@/store/useTranscriptionStore";
import { TranscriptionView } from "@/views/TranscriptionView";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const TITLES: Record<TranscriptionTab, string> = { meeting: "会议", transcribe: "文件", settings: "识别" };

const ITEMS: NodeItem<TranscriptionTab>[] = [
  { key: "meeting", label: TITLES.meeting, icon: <Users {...iconProps} /> },
  { key: "transcribe", label: TITLES.transcribe, icon: <FileAudio {...iconProps} /> },
  { key: "settings", label: TITLES.settings, icon: <SlidersHorizontal {...iconProps} /> },
];

export function TranscriptionPage() {
  const { tab, setTab } = useTranscriptionStore();
  return (
    <MasterDetail wide={tab !== "settings"} title="转写" list={<NodeList ariaLabel="转写分类" items={ITEMS} active={tab} onSelect={setTab} />}>
      <InspectorHeader title={TITLES[tab]} />
      <TranscriptionView />
    </MasterDetail>
  );
}
