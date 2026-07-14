import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { Input, Select, Textarea } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import { CMD, cmd } from "@/lib/tauri";
import {
  SMART_TEXT_PLACEHOLDER,
  useDictPrefs,
  type SmartTextTemplate,
} from "@/store/useDictPrefs";

const PREVIEW_SAMPLE = "嗯，我我觉得这个方案其实可以再简单一点，然后明天发给大家。";

export function SmartTextPanel() {
  const prefs = useDictPrefs((state) => state.prefs);
  const patch = useDictPrefs((state) => state.patch);
  const templates = prefs.smartTemplates;
  const active = templates.find((template) => template.id === prefs.smartTemplateId) ?? templates[0];
  const [previewInput, setPreviewInput] = useState(PREVIEW_SAMPLE);
  const [previewOutput, setPreviewOutput] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const [message, setMessage] = useState("");

  const updateTemplate = (partial: Partial<SmartTextTemplate>) => {
    if (!active) return;
    void patch({
      smartTemplates: templates.map((template) =>
        template.id === active.id ? { ...template, ...partial } : template,
      ),
    });
  };

  const addTemplate = () => {
    const template: SmartTextTemplate = {
      id: crypto.randomUUID(),
      name: "新模板",
      prompt: `请优化下面的语音识别文本，只输出处理结果。\n\n${SMART_TEXT_PLACEHOLDER}`,
    };
    void patch({ smartTemplates: [...templates, template], smartTemplateId: template.id });
  };

  const deleteTemplate = () => {
    if (!active || templates.length <= 1) return;
    const next = templates.filter((template) => template.id !== active.id);
    void patch({ smartTemplates: next, smartTemplateId: next[0].id });
  };

  const preview = async () => {
    if (!active) return;
    setPreviewing(true);
    setMessage("");
    try {
      const output = await cmd<string>(CMD.previewSmartText, {
        text: previewInput,
        prompt: active.prompt,
      });
      setPreviewOutput(output);
    } catch (error) {
      setMessage(`试运行失败：${String(error)}`);
    } finally {
      setPreviewing(false);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection
        title="智能处理"
        right={<Switch
          checked={prefs.smartProcessingEnabled}
          onChange={(value) => void patch({ smartProcessingEnabled: value })}
          label="启用智能处理"
        />}
      >
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          识别结束后先执行本地处理，再把文本交给默认大语言模型，最终注入模型返回的内容。
        </p>
      </SettingsSection>

      <SettingsSection title="处理模板">
        <Field
          label="当前模板"
          controlId="smart-text-template"
          actions={
            <>
              <Button onClick={addTemplate}>+ 新建</Button>
              <Button variant="danger" disabled={templates.length <= 1} onClick={deleteTemplate}>删除</Button>
            </>
          }
        >
            <Select
              id="smart-text-template"
              value={active?.id ?? ""}
              onChange={(event) => void patch({ smartTemplateId: event.target.value })}
            >
              {templates.map((template) => <option key={template.id} value={template.id}>{template.name}</option>)}
            </Select>
        </Field>

        {active && (
          <div className="mt-1 flex flex-col gap-4">
            <Field label="模板名称">
              <Input value={active.name} onChange={(event) => updateTemplate({ name: event.target.value })} />
            </Field>
            <Field
              label="提示词"
              hint={<>使用 <code className="text-[var(--color-accent-light)]">{SMART_TEXT_PLACEHOLDER}</code> 表示识别文本；启用时必须保留该占位符。</>}
            >
              <Textarea
                rows={7}
                value={active.prompt}
                spellCheck={false}
                onChange={(event) => updateTemplate({ prompt: event.target.value })}
              />
            </Field>
            <Button
              size="sm"
              className="self-start"
              onClick={() => updateTemplate({ prompt: `${active.prompt}${active.prompt.endsWith("\n") ? "" : "\n"}${SMART_TEXT_PLACEHOLDER}` })}
            >
              插入文本占位符
            </Button>
          </div>
        )}
      </SettingsSection>

      <SettingsSection title="试运行">
        <FormGrid className="gap-y-3">
          <Field label="试运行 · 输入">
            <Textarea rows={4} value={previewInput} onChange={(event) => setPreviewInput(event.target.value)} />
          </Field>
          <Field label="试运行 · 输出">
            <Textarea rows={4} readOnly value={previewOutput} className="bg-[var(--color-bg)]" />
          </Field>
        </FormGrid>
        <div className="flex items-center gap-3">
          <Button size="sm" variant="primary" disabled={previewing || !active} onClick={preview}>
            {previewing ? "处理中..." : "使用默认模型试运行"}
          </Button>
          {message && <p className="text-xs text-[var(--color-err)]">{message}</p>}
        </div>
      </SettingsSection>
    </div>
  );
}
