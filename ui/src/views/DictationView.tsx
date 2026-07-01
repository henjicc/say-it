import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Textarea } from "@/components/ui/Input";
import { CheckField } from "@/components/ui/Field";
import { LogPanel } from "@/components/ui/LogPanel";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { LocalRulesPanel } from "@/views/LocalRulesPanel";
import { DictationShortcutsPanel } from "@/views/DictationShortcutsPanel";
import { FunAsrHotwordsPanel } from "@/views/FunAsrHotwordsPanel";
import { cn } from "@/lib/cn";
import { useDictationStore } from "@/store/useDictationStore";
import { useDictPrefs } from "@/store/useDictPrefs";
import { toggleDictation, clearDictLog } from "@/features/dictation/controller";

const toneClass: Record<string, string> = {
  "": "text-white/60",
  ok: "text-[#25c36f]",
  err: "text-[#ff6b6b]",
};

type TabKey = "basic" | "local" | "hotwords" | "debug";

const TABS: TabItem<TabKey>[] = [
  { key: "basic", label: "快捷键" },
  { key: "local", label: "本地处理" },
  { key: "hotwords", label: "热词" },
  { key: "debug", label: "调试" },
];

export function DictationView() {
  const [tab, setTab] = useState<TabKey>("basic");
  const { statusText, statusTone, latestText, log, recording } = useDictationStore();
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);

  return (
    <div className="flex flex-col gap-4 py-2">
      <div>
        <h1 className="text-xl font-semibold text-white">语音输入</h1>
        <p className="mt-1 text-sm text-white/45">
          按快捷键说话，再次按下停止并注入到当前光标位置。
        </p>
      </div>

      <Tabs<TabKey> tabs={TABS} active={tab} onChange={setTab} />

      {tab === "basic" && <DictationShortcutsPanel />}
      {tab === "local" && <LocalRulesPanel />}
      {tab === "hotwords" && <FunAsrHotwordsPanel />}
      {tab === "debug" && (
        <div className="mt-4">
          <Button variant={recording ? "danger" : "primary"} onClick={toggleDictation}>
            {recording ? "停止并注入" : "手动开始"}
          </Button>
          <p className={cn("mt-2 text-sm", toneClass[statusTone])}>{statusText}</p>

          <div className="mt-4 rounded-xl border border-white/10 bg-white/[0.03] p-4">
            <p className="text-sm font-medium text-white">最近识别</p>
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
  );
}
