import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Textarea } from "@/components/ui/Input";
import { CheckField } from "@/components/ui/Field";
import { LogPanel } from "@/components/ui/LogPanel";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { LocalRulesPanel } from "@/views/LocalRulesPanel";
import { SmartTextPanel } from "@/views/SmartTextPanel";
import { DictationShortcutsPanel } from "@/views/DictationShortcutsPanel";
import { cn } from "@/lib/cn";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { toggleDictation, clearDictLog } from "@/features/dictation/controller";

const toneClass: Record<string, string> = {
  "": "text-[var(--color-fg-muted)]",
  ok: "text-[var(--color-ok)]",
  err: "text-[var(--color-err)]",
};

type TabKey = "basic" | "local" | "smart" | "debug";

const TABS: TabItem<TabKey>[] = [
  { key: "basic", label: "通用设置" },
  { key: "local", label: "本地处理" },
  { key: "smart", label: "智能处理" },
  { key: "debug", label: "调试" },
];

export function DictationView() {
  const [tab, setTab] = useState<TabKey>("basic");
  const { statusText, statusTone, latestText, log, recording } = useDictationStore();
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="语音输入"
        description="按快捷键说话，再次按下停止并注入到当前光标位置。"
      />

      <Tabs<TabKey>
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
        {tab === "smart" && <SmartTextPanel />}
        {tab === "debug" && (
          <div className="flex flex-col gap-3">
          <div>
            <Button variant={recording ? "danger" : "primary"} onClick={toggleDictation}>
              {recording ? "停止并注入" : "手动开始"}
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
              <CheckField checked={prefs.debugLog} onChange={(v) => patch({ debugLog: v })}>
                输出调试日志
              </CheckField>
            </div>
            <LogPanel className="mt-2">{log}</LogPanel>
          </div>
          </div>
        )}
      </div>
    </div>
  );
}
