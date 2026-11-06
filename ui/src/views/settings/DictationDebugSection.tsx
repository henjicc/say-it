import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Group, Row } from "@/components/ui/Group";
import { Textarea } from "@/components/ui/Input";
import { LogPanel } from "@/components/ui/LogPanel";
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
    <Group title="听写调试">
      <Row
        label="当前软件上下文"
        hint={`打开预览窗口，切换到要查看的软件，再按 ${contextDebugShortcutLabel}${contextDebugShortcutHint}，即可查看能读取到的文字。预览不会录音、调用智能模型或保存内容。`}
        message={notice && <span role="status">{notice}</span>}
      >
        <Button size="sm" disabled={opening} onClick={() => void openContextDebug()}>{opening ? "正在打开…" : "打开调试窗口"}</Button>
      </Row>
      <Row label="手动听写" message={<span role="status" className={cn(toneClass[statusTone])}>{statusText}</span>}>
        <Button size="sm" variant={recording ? "danger" : "primary"} onClick={toggleDictation}>{recording ? "停止并输入" : "手动开始"}</Button>
      </Row>
      <div className="flex flex-col gap-2 p-3.5">
        <Textarea rows={3} readOnly aria-label="最近识别" value={latestText} placeholder="最近一次识别的完整文本会显示在这里" />
        <div><Button size="sm" onClick={clearDictLog}>清空日志</Button></div>
        <LogPanel>{log}</LogPanel>
      </div>
    </Group>
  );
}
