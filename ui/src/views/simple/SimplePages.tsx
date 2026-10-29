import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Group, Row } from "@/components/ui/Group";
import { Switch } from "@/components/ui/Switch";
import { Tabs } from "@/components/ui/Tabs";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { useThemeStore } from "@/store/useThemeStore";
import { useTranscriptionStore } from "@/store/useTranscriptionStore";
import { useUiStore } from "@/store/useUiStore";
import { AsrModelRow, MicRow, ShortcutRows } from "@/views/dictation/DictationSections";
import { SubtitleAsrRow, SubtitleOutputRow, SubtitleSourceRow } from "@/views/subtitles/SubtitleSections";
import { SubtitleRunButton, useSubtitlePreviewLifecycle } from "@/views/subtitles/SubtitlesPage";
import { TranscriptionView } from "@/views/TranscriptionView";
import { HistoryView } from "@/views/HistoryView";
import { useStartupSettings } from "@/views/SettingsStartupPanel";
import { useHistoryPrefs } from "@/views/SettingsHistoryPanel";

/** 简单模式内容区：窄窗口里只放最必要的几项，其余设置照常按专业模式里的值生效。 */
function SimpleBody({ children }: { children: React.ReactNode }) {
  return <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-4">{children}</div>;
}

export function SimpleDictation() {
  const enabled = useDictPrefs((s) => s.prefs.smartProcessingEnabled);
  const patch = useDictPrefs((s) => s.patch);
  return (
    <SimpleBody>
      <Group><ShortcutRows /></Group>
      <Group><MicRow /><AsrModelRow /></Group>
      <Group>
        <Row label="智能润色" hint="识别结束后用智能模型整理文字；模板与模型在专业模式里调整。">
          <Switch checked={enabled} onChange={(value) => void patch({ smartProcessingEnabled: value })} label="智能润色" />
        </Row>
      </Group>
    </SimpleBody>
  );
}

export function SimpleSubtitles() {
  useSubtitlePreviewLifecycle();
  return (
    <SimpleBody>
      <Group><SubtitleSourceRow /><SubtitleAsrRow /><SubtitleOutputRow /></Group>
      <div className="flex-1" />
      <SubtitleRunButton className="h-9 w-full" />
    </SimpleBody>
  );
}

export function SimpleTranscription() {
  const setTab = useTranscriptionStore((s) => s.setTab);
  useEffect(() => setTab("transcribe"), [setTab]);
  return <SimpleBody><TranscriptionView /></SimpleBody>;
}

export function SimpleHistory() {
  return <SimpleBody><HistoryView /></SimpleBody>;
}

export function SimpleSettings() {
  const { startup, startupMsg, saveStartup } = useStartupSettings();
  const { prefs: historyPrefs, save: saveHistory } = useHistoryPrefs();
  const cueEnabled = useDictPrefs((s) => s.prefs.cueEnabled);
  const patchDict = useDictPrefs((s) => s.patch);
  const tone = useThemeStore((s) => s.theme.tone);
  const patchTheme = useThemeStore((s) => s.patch);
  const configured = useProviderStore((s) => s.profiles)
    .filter((profile) => profile.enabled && profile.status?.hasApiKey)
    .map((profile) => profile.displayName);
  // 管理密钥需要专业模式的完整界面：放大窗口后自动切到专业模式。
  const manageKeys = () => {
    useUiStore.getState().openModels("asr");
    void getCurrentWindow().maximize();
  };

  return (
    <SimpleBody>
      <Group>
        <Row label="服务密钥">
          <button type="button" onClick={manageKeys} className="max-w-48 truncate rounded-[var(--radius-sm)] px-2 py-1 text-[13px] text-[var(--color-fg-muted)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-fg)]">
            {configured.length ? configured.join("、") : "去配置"}
          </button>
        </Row>
      </Group>
      <Group>
        <Row label="开机自启" message={startupMsg && <span>{startupMsg}</span>}>
          <Switch checked={!!startup.autostart} onChange={(value) => void saveStartup({ autostart: value, silentStart: startup.silentStart }, value ? "已开启开机自启。" : "已关闭开机自启。")} label="开机自启" />
        </Row>
        <Row label="提示音">
          <Switch checked={cueEnabled} onChange={(value) => void patchDict({ cueEnabled: value })} label="提示音" />
        </Row>
        <Row label="保存历史">
          <Switch checked={historyPrefs.enabled} onChange={(enabled) => void saveHistory({ ...historyPrefs, enabled })} label="保存历史" />
        </Row>
      </Group>
      <Group>
        <Row label="外观">
          <Tabs
            variant="view"
            ariaLabel="外观"
            tabs={[{ key: "dark", label: "深色" }, { key: "light", label: "浅色" }]}
            active={tone}
            onChange={(value) => patchTheme({ tone: value as "dark" | "light" })}
          />
        </Row>
      </Group>
    </SimpleBody>
  );
}
