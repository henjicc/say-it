import { useEffect } from "react";
import { Languages, RadioTower, SlidersHorizontal, Type } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Group } from "@/components/ui/Group";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { TRANSLATION_MODEL_NONE } from "@/features/translation/models";
import { cn } from "@/lib/cn";
import { hideSubtitlePreview, showSubtitlePreview, syncSubtitleIndicator, toggleSubtitles } from "@/features/subtitles/controller";
import { useSubtitleStore } from "@/store/useSubtitleStore";
import { useUiStore, type SubtitleNodeKey } from "@/store/useUiStore";
import { SubtitleStylePanel } from "@/views/SubtitleStylePanel";
import { SubtitleTranslationPanel } from "@/views/SubtitleTranslationPanel";
import { ObsOverlayPanel } from "@/views/ObsOverlayPanel";
import { SubtitleAsrRow, SubtitleModeRow, SubtitleOutputRow, SubtitleShortcutRow, SubtitleSourceRow } from "./SubtitleSections";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const TITLES: Record<SubtitleNodeKey, string> = {
  basics: "基础",
  translation: "翻译",
  style: "样式",
  output: "输出",
};

/** 字幕运行与预览的生命周期由 Rust 持有；本页只负责样式草稿的即时同步和离开时关闭预览。 */
export function useSubtitlePreviewLifecycle() {
  const previewOpen = useSubtitleStore((s) => s.previewActive);
  const running = useSubtitleStore((s) => s.running);
  const prefs = useSubtitleStore((s) => s.prefs);
  useEffect(() => {
    if (running || previewOpen) syncSubtitleIndicator(prefs);
  }, [prefs, running, previewOpen]);
  useEffect(() => () => { void hideSubtitlePreview(); }, []);
}

export function SubtitleRunButton({ className }: { className?: string }) {
  const running = useSubtitleStore((s) => s.running);
  return (
    <Button variant={running ? "danger" : "primary"} className={className} onClick={toggleSubtitles}>
      {running ? "停止字幕" : "开始字幕"}
    </Button>
  );
}

export function SubtitlesPage() {
  useSubtitlePreviewLifecycle();
  const node = useUiStore((s) => s.subtitleNode);
  const setNode = useUiStore((s) => s.setSubtitleNode);
  const { prefs, running, previewActive } = useSubtitleStore();

  const items: NodeItem<SubtitleNodeKey>[] = [
    { key: "basics", label: TITLES.basics, icon: <SlidersHorizontal {...iconProps} /> },
    { key: "translation", label: TITLES.translation, icon: <Languages {...iconProps} />, off: prefs.translationModel === TRANSLATION_MODEL_NONE },
    { key: "style", label: TITLES.style, icon: <Type {...iconProps} /> },
    { key: "output", label: TITLES.output, icon: <RadioTower {...iconProps} /> },
  ];

  return (
    <MasterDetail
      title="字幕"
      titleRight={<span className={cn("text-xs", running ? "text-[var(--color-accent-text)]" : "text-[var(--color-fg-subtle)]")}>{running ? "运行中" : "未运行"}</span>}
      list={<NodeList ariaLabel="字幕设置" items={items} active={node} onSelect={setNode} />}
      listFooter={<>
        <Button
          aria-pressed={previewActive}
          disabled={running}
          onClick={() => { void (previewActive ? hideSubtitlePreview() : showSubtitlePreview(prefs)); }}
        >
          {previewActive ? "关闭预览" : "预览"}
        </Button>
        <SubtitleRunButton />
      </>}
    >
      <InspectorHeader title={TITLES[node]} />
      {node === "basics" && (
        <InspectorBody>
          <Group title="触发"><SubtitleShortcutRow /></Group>
          <Group title="识别"><SubtitleSourceRow /><SubtitleAsrRow /></Group>
        </InspectorBody>
      )}
      {node === "translation" && <SubtitleTranslationPanel />}
      {node === "style" && <InspectorBody><Group><SubtitleModeRow /></Group><SubtitleStylePanel /></InspectorBody>}
      {node === "output" && <InspectorBody><Group><SubtitleOutputRow /></Group><ObsOverlayPanel /></InspectorBody>}
    </MasterDetail>
  );
}
