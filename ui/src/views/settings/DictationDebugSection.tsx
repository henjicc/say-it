import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Textarea } from "@/components/ui/Input";
import { LogPanel } from "@/components/ui/LogPanel";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { cn } from "@/lib/cn";
import { contextDebugShortcutHint, contextDebugShortcutLabel } from "@/lib/platform";
import { useDictationStore } from "@/store/useDictationStore";
import { toggleDictation, clearDictLog } from "@/features/dictation/controller";
import { CMD, cmd } from "@/lib/tauri";

const toneClass: Record<string, string> = {
  "": "text-[var(--color-fg-muted)]",
  ok: "text-[var(--color-ok)]",
  err: "text-[var(--color-err)]",
};

/** 开发构建专用：当前软件上下文预览、手动听写与日志。 */
export function DictationDebugSection() {
  const { statusText, statusTone, latestText, log, recording } = useDictationStore();
  const [opening, setOpening] = useState(false);
  const [notice, setNotice] = useState("");
  if (!import.meta.env.DEV) return null;

  const openContextDebug = async () => {
    setOpening(true);
    setNotice("");
    try {
      await cmd(CMD.openActiveAppContextDebug);
      setNotice("调试窗口已打开，快捷键仅在窗口打开期间生效。");
    } catch (error) {
      setNotice(`打开失败：${String(error)}`);
    } finally {
      setOpening(false);
    }
  };

  return (
    <SettingsSection title="听写调试" description={`打开预览窗口，切换到要查看的软件，再按 ${contextDebugShortcutLabel}${contextDebugShortcutHint}，即可查看能读取到的文字。预览不会录音、调用智能模型或保存内容。`}>
      <div className="flex flex-wrap items-center gap-3">
        <Button disabled={opening} onClick={() => void openContextDebug()}>
          {opening ? "正在打开…" : "打开上下文调试窗口"}
        </Button>
        <Button variant={recording ? "danger" : "primary"} onClick={toggleDictation}>
          {recording ? "停止并输入" : "手动开始"}
        </Button>
        {notice && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{notice}</p>}
      </div>
      <p className={cn("text-sm", toneClass[statusTone])}>{statusText}</p>
      <Textarea rows={3} readOnly value={latestText} placeholder="最近一次识别的完整文本会显示在这里" />
      <div><Button size="sm" onClick={clearDictLog}>清空日志</Button></div>
      <LogPanel>{log}</LogPanel>
    </SettingsSection>
  );
}
