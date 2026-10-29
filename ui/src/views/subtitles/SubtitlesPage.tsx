import { useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Group } from "@/components/ui/Group";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { SUBTITLE_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { TRANSLATION_MODEL_NONE } from "@/features/translation/models";
import { hideSubtitlePreview, showSubtitlePreview, syncSubtitleIndicator, toggleSubtitles } from "@/features/subtitles/controller";
import { parseSubtitleSource, useSubtitleStore } from "@/store/useSubtitleStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useUiStore, type SubtitleNodeKey } from "@/store/useUiStore";
import { SubtitleStylePanel } from "@/views/SubtitleStylePanel";
import { SubtitleTranslationPanel } from "@/views/SubtitleTranslationPanel";
import { ObsOverlayPanel } from "@/views/ObsOverlayPanel";
import { SubtitleAsrRow, SubtitleModeRow, SubtitleOutputRow, SubtitleShortcutRow, SubtitleSourceRow } from "./SubtitleSections";

const TITLES: Record<SubtitleNodeKey, string> = {
  trigger: "触发",
  audio: "声音",
  asr: "识别",
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
  useModelCatalogRevision();
  useSubtitlePreviewLifecycle();
  const node = useUiStore((s) => s.subtitleNode);
  const setNode = useUiStore((s) => s.setSubtitleNode);
  const { prefs, running, previewActive, shortcutLabel } = useSubtitleStore();
  const micDeviceId = useDictPrefs((s) => s.prefs.micDeviceId);

  const source = parseSubtitleSource(prefs.source);
  const sourceLabel = source.kind === "system" ? (source.deviceName || "系统音频") : (micDeviceId || "系统默认麦克风");
  const asrLabel = SUBTITLE_ASR_MODEL_OPTIONS.find((item) => item.value === prefs.asrModel)?.label ?? prefs.asrModel;
  const translating = prefs.translationModel !== TRANSLATION_MODEL_NONE;

  const items: NodeItem<SubtitleNodeKey>[] = [
    { key: "trigger", label: "触发", value: shortcutLabel || "手动开始" },
    { key: "audio", label: "声音", value: sourceLabel },
    { key: "asr", label: "识别", value: asrLabel },
    { key: "translation", label: "翻译", value: translating ? "已开启" : "不翻译", state: translating ? "on" : "off" },
    { key: "style", label: "样式", value: prefs.mode === "replace" ? "单句替换" : "滚动累积" },
    { key: "output", label: "输出", value: prefs.obsOutputEnabled ? "OBS" : "桌面" },
  ];

  return (
    <MasterDetail
      title="字幕"
      titleRight={<span className="text-xs text-[var(--color-fg-subtle)]">{running ? "运行中" : "未运行"}</span>}
      list={<NodeList chain ariaLabel="字幕流程" items={items} active={node} onSelect={setNode} />}
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
      {node === "trigger" && <InspectorBody><Group><SubtitleShortcutRow /></Group></InspectorBody>}
      {node === "audio" && <InspectorBody><Group><SubtitleSourceRow /></Group></InspectorBody>}
      {node === "asr" && <InspectorBody><Group><SubtitleAsrRow /></Group></InspectorBody>}
      {node === "translation" && <SubtitleTranslationPanel />}
      {node === "style" && <InspectorBody><Group><SubtitleModeRow /></Group><SubtitleStylePanel /></InspectorBody>}
      {node === "output" && <InspectorBody><Group><SubtitleOutputRow /></Group><ObsOverlayPanel /></InspectorBody>}
    </MasterDetail>
  );
}
