import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Textarea } from "@/components/ui/Input";
import { LogPanel } from "@/components/ui/LogPanel";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { LocalRulesPanel } from "@/views/LocalRulesPanel";
import { SceneRulesPanel } from "@/views/SceneRulesPanel";
import { DictationShortcutsPanel } from "@/views/DictationShortcutsPanel";
import { cn } from "@/lib/cn";
import { contextDebugShortcutHint, contextDebugShortcutLabel } from "@/lib/platform";
import { useDictationStore } from "@/store/useDictationStore";
import { toggleDictation, clearDictLog } from "@/features/dictation/controller";
import { CMD, cmd } from "@/lib/tauri";
import { useUiStore, type DictationTabKey } from "@/store/useUiStore";

const toneClass: Record<string, string> = {
  "": "text-[var(--color-fg-muted)]",
  ok: "text-[var(--color-ok)]",
  err: "text-[var(--color-err)]",
};

const TABS: TabItem<DictationTabKey>[] = [
  { key: "basic", label: "通用设置" },
  { key: "local", label: "本地处理" },
  { key: "apps", label: "场景规则" },
  ...(import.meta.env.DEV ? [{ key: "debug" as const, label: "调试" }] : []),
];

export function DictationView() {
  const tab = useUiStore((state) => state.dictationTab);
  const setTab = useUiStore((state) => state.setDictationTab);
  const { statusText, statusTone, latestText, log, recording } = useDictationStore();
  const [contextDebugOpening, setContextDebugOpening] = useState(false);
  const [contextDebugNotice, setContextDebugNotice] = useState("");

  const openContextDebug = async () => {
    setContextDebugOpening(true);
    setContextDebugNotice("");
    try {
      await cmd(CMD.openActiveAppContextDebug);
      setContextDebugNotice("调试窗口已打开，快捷键仅在窗口打开期间生效。");
    } catch (error) {
      setContextDebugNotice(`打开失败：${String(error)}`);
    } finally {
      setContextDebugOpening(false);
    }
  };

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="语音输入"
        description="按快捷键开始说话，再按一次结束，文字会自动输入到光标位置。"
      />

      <Tabs<DictationTabKey>
        id="dictation-tabs"
        ariaLabel="语音输入设置"
        tabs={TABS}
        active={tab}
        onChange={setTab}
      />

      <div
        id={`dictation-tabs-${tab}-panel`}
        role="tabpanel"
        aria-labelledby={`dictation-tabs-${tab}-tab`}
      >
        {tab === "basic" && <DictationShortcutsPanel />}
        {tab === "local" && <LocalRulesPanel />}
        {tab === "apps" && <SceneRulesPanel />}
        {tab === "debug" && (
          <div className="flex flex-col gap-7">
            <SettingsSection title="当前软件上下文调试" description={`打开预览窗口，切换到要查看的软件，再按 ${contextDebugShortcutLabel}${contextDebugShortcutHint}，即可查看能读取到的文字。预览不会录音、调用智能模型或保存内容。`}>
              <div className="flex flex-wrap items-center gap-3">
                <Button variant="primary" disabled={contextDebugOpening} onClick={() => void openContextDebug()}>
                  {contextDebugOpening ? "正在打开…" : "打开上下文调试窗口"}
                </Button>
                {contextDebugNotice && (
                  <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{contextDebugNotice}</p>
                )}
              </div>
            </SettingsSection>
            <div>
              <Button variant={recording ? "danger" : "primary"} onClick={toggleDictation}>
                {recording ? "停止并输入" : "手动开始"}
              </Button>
              <p className={cn("mt-2 text-sm", toneClass[statusTone])}>{statusText}</p>
            </div>

            <div className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
              <p className="text-sm font-medium text-[var(--color-fg)]">最近识别</p>
              <Textarea
                className="mt-3"
                rows={3}
                readOnly
                value={latestText}
                placeholder="最近一次识别的完整文本会显示在这里"
              />
              <div className="mt-2.5 flex items-center gap-3">
                <Button size="sm" onClick={clearDictLog}>
                  清空日志
                </Button>
              </div>
              <LogPanel className="mt-2">{log}</LogPanel>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
