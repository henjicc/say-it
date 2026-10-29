import { InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { FILE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { useTranscriptionStore, type TranscriptionTab } from "@/store/useTranscriptionStore";
import { TranscriptionView } from "@/views/TranscriptionView";

const TITLES: Record<TranscriptionTab, string> = { transcribe: "文件", settings: "识别", align: "文稿对齐" };

export function TranscriptionPage() {
  useModelCatalogRevision();
  const { tab, setTab, selectedFile, params } = useTranscriptionStore();
  const modelLabel = FILE_ASR_MODEL_OPTIONS.find((item) => item.value === params.model)?.label ?? params.model;
  const items: NodeItem<TranscriptionTab>[] = [
    { key: "transcribe", label: "文件", value: selectedFile?.name ?? "未选择" },
    { key: "settings", label: "识别", value: modelLabel || "未选择模型" },
    { key: "align", label: "文稿对齐", value: "按文稿生成时间轴" },
  ];
  return (
    <MasterDetail title="转写" list={<NodeList chain ariaLabel="转写流程" items={items} active={tab} onSelect={setTab} />}>
      <InspectorHeader title={TITLES[tab]} />
      <TranscriptionView />
    </MasterDetail>
  );
}
