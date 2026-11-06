import { Activity, AudioWaveform, History, Info, Keyboard, LayoutGrid, Volume2 } from "lucide-react";
import { InspectorBody, InspectorHeader, MasterDetail, NodeList, type NodeItem } from "@/components/layout/MasterDetail";
import { SettingsSearch, useSearchHitHighlight } from "@/features/settings/SettingsSearch";
import { useUiStore, type SettingsSectionKey } from "@/store/useUiStore";
import { SettingsStartupPanel } from "@/views/SettingsStartupPanel";
import { SettingsSetupPanel } from "@/views/SettingsSetupPanel";
import { SettingsAppearancePanel } from "@/views/SettingsAppearancePanel";
import { SettingsMicCuePanel } from "@/views/SettingsMicCuePanel";
import { SettingsKeyBindingsPanel } from "@/views/SettingsKeyBindingsPanel";
import { SettingsHistoryPanel } from "@/views/SettingsHistoryPanel";
import { AudioProcessingPanel, DataResetSection, DiagnosticSection } from "@/views/SettingsAdvancedPanel";
import { DictationDebugSection } from "@/views/settings/DictationDebugSection";

const iconProps = { strokeWidth: 1.6, "aria-hidden": true } as const;

const ITEMS: NodeItem<SettingsSectionKey>[] = [
  { key: "general", label: "通用", icon: <LayoutGrid {...iconProps} /> },
  { key: "sound", label: "声音", icon: <Volume2 {...iconProps} /> },
  { key: "processing", label: "音频处理", icon: <AudioWaveform {...iconProps} /> },
  { key: "keys", label: "快捷键", icon: <Keyboard {...iconProps} /> },
  { key: "history", label: "历史与学习", icon: <History {...iconProps} /> },
  { key: "diagnostics", label: "诊断与数据", icon: <Activity {...iconProps} /> },
];

const PANEL_ID = "settings-section-panel";

export function SettingsPage() {
  const section = useUiStore((s) => s.settingsSection);
  const setSection = useUiStore((s) => s.setSettingsSection);
  const goToSearchHit = useUiStore((s) => s.goToSearchHit);
  const openAbout = useUiStore((s) => s.openAbout);
  useSearchHitHighlight("settings", section, PANEL_ID);

  return (
    <MasterDetail
      title="设置"
      list={
        <div className="flex flex-col gap-3">
          <SettingsSearch onSelect={(item) => goToSearchHit({ id: `${item.id}:${Date.now()}`, view: item.target.view, section: item.target.section, targetText: item.targetText })} />
          <NodeList ariaLabel="设置分类" items={ITEMS} active={section} onSelect={setSection} />
        </div>
      }
      listFooter={
        <button
          type="button"
          onClick={openAbout}
          className="flex h-10 items-center gap-2.5 rounded-[var(--radius-md)] px-4 text-sm text-[var(--color-fg-subtle)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]"
        >
          <Info className="h-4 w-4" {...iconProps} />关于
        </button>
      }
    >
      <InspectorHeader title={ITEMS.find((item) => item.key === section)?.label} />
      <div id={PANEL_ID}>
        {section === "general" && <InspectorBody className="gap-8"><SettingsStartupPanel /><SettingsAppearancePanel /><SettingsSetupPanel /></InspectorBody>}
        {section === "sound" && <SettingsMicCuePanel />}
        {section === "processing" && <AudioProcessingPanel />}
        {section === "keys" && <SettingsKeyBindingsPanel />}
        {section === "history" && <SettingsHistoryPanel />}
        {section === "diagnostics" && <InspectorBody className="gap-8"><DiagnosticSection /><DictationDebugSection /><DataResetSection /></InspectorBody>}
      </div>
    </MasterDetail>
  );
}
