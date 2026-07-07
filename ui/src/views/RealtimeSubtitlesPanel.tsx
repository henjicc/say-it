import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { toggleSubtitles } from "@/features/subtitles/controller";
import { useSubtitleStore } from "@/store/useSubtitleStore";
import { SubtitleGeneralPanel } from "@/views/SubtitleGeneralPanel";
import { SubtitleStylePanel } from "@/views/SubtitleStylePanel";
import { SubtitleTranslationPanel } from "@/views/SubtitleTranslationPanel";

type TabKey = "general" | "style" | "translation";

const TABS: TabItem<TabKey>[] = [
  { key: "general", label: "通用设置" },
  { key: "style", label: "字幕样式" },
  { key: "translation", label: "字幕翻译" },
];

export function RealtimeSubtitlesPanel() {
  const [tab, setTab] = useState<TabKey>("general");
  const running = useSubtitleStore((s) => s.running);

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="实时字幕"
        description="持续识别语音并在屏幕上显示字幕，适合会议、网课、视频和临时转写。"
        actions={
          <Button variant={running ? "danger" : "primary"} onClick={toggleSubtitles}>
            {running ? "停止字幕" : "开始字幕"}
          </Button>
        }
      />

      <Tabs<TabKey> tabs={TABS} active={tab} onChange={setTab} />

      {tab === "general" && <SubtitleGeneralPanel />}
      {tab === "style" && <SubtitleStylePanel />}
      {tab === "translation" && <SubtitleTranslationPanel />}
    </div>
  );
}
