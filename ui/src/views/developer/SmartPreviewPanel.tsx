import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Group } from "@/components/ui/Group";
import { Textarea } from "@/components/ui/Input";
import { CMD, cmd } from "@/lib/tauri";
import { ACTIVE_APP_CONTEXT_PLACEHOLDER, useDictPrefs } from "@/store/useDictPrefs";

const SAMPLE_INPUT = "嗯，我我觉得这个方案其实可以再简单一点，然后明天发给大家。";
const SAMPLE_CONTEXT = "应用：Visual Studio Code\n窗口：方案说明.md\n窗口可见文字：Tauri；OCR；上下文捕获";

/** 用当前的智能模型和模板试跑一段口述原文；只显示结果，不输入到别的软件，也不进历史。 */
export function SmartPreviewPanel() {
  const prefs = useDictPrefs((state) => state.prefs);
  const template = prefs.smartTemplates.find((item) => item.id === prefs.smartTemplateId) ?? prefs.smartTemplates[0];
  const [input, setInput] = useState(SAMPLE_INPUT);
  const [context, setContext] = useState(SAMPLE_CONTEXT);
  const [output, setOutput] = useState("");
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState("");
  const usesContext = Boolean(template?.prompt.includes(ACTIVE_APP_CONTEXT_PLACEHOLDER));

  const run = async () => {
    if (!template) return;
    setRunning(true);
    setMessage("");
    try {
      setOutput(await cmd<string>(CMD.previewSmartText, {
        text: input,
        prompt: template.prompt,
        providerId: prefs.smartLlmProviderId,
        model: prefs.smartLlmModel,
        activeAppContext: usesContext ? context : undefined,
      }));
    } catch (error) {
      setMessage(`试运行失败：${String(error)}`);
    } finally {
      setRunning(false);
    }
  };

  return (
    <Group
      title="模板试运行"
      right={<Button size="sm" variant="primary" disabled={running || !template} onClick={() => void run()}>{running ? "处理中…" : "试运行"}</Button>}
    >
      <p className="px-3.5 pt-3 text-xs text-[var(--color-fg-muted)]">用语音输入当前的智能模型和模板「{template?.name ?? ""}」处理下面的原文，可能产生费用。</p>
      <label className="flex flex-col gap-2 p-3.5">
        <span className="text-xs text-[var(--color-fg-subtle)]">输入</span>
        <Textarea rows={4} value={input} onChange={(event) => setInput(event.target.value)} />
      </label>
      {usesContext && (
        <label className="flex flex-col gap-2 p-3.5">
          <span className="text-xs text-[var(--color-fg-subtle)]">模拟当前软件上下文</span>
          <Textarea rows={4} value={context} onChange={(event) => setContext(event.target.value)} />
        </label>
      )}
      <label className="flex flex-col gap-2 p-3.5">
        <span className="text-xs text-[var(--color-fg-subtle)]">输出</span>
        <Textarea rows={4} readOnly value={output} />
      </label>
      {message && <p role="alert" className="px-3.5 pb-3 text-xs text-[var(--color-err)]">{message}</p>}
    </Group>
  );
}
