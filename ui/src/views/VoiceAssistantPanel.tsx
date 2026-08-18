import { useEffect, useState } from "react";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { optionsForScene, useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { TRANSLATION_MODEL_OPTIONS } from "@/features/translation/models";
import { TRANSLATION_SOURCE_LANGUAGE_OPTIONS, TRANSLATION_TARGET_LANGUAGE_OPTIONS } from "@/features/translation/languages";
import { CMD, cmd, type AppSnapshot } from "@/lib/tauri";

interface AssistantPrefs { translationModel: string; sourceLanguage: string; targetLanguage: string }
const defaults: AssistantPrefs = { translationModel: "none", sourceLanguage: "auto", targetLanguage: "zh" };

export function VoiceAssistantPanel() {
  useModelCatalogRevision();
  const [prefs, setPrefs] = useState(defaults);
  const [message, setMessage] = useState("");
  const pluginOptions = optionsForScene("subtitleTranslation");
  useEffect(() => { void cmd<AppSnapshot>(CMD.getAppSnapshot).then((snapshot) => setPrefs({ ...defaults, ...snapshot.settings.assistantPrefs } as AssistantPrefs)).catch((error) => setMessage(String(error))); }, []);
  async function save(next: AssistantPrefs) { setPrefs(next); try { await cmd(CMD.updateAppSettings, { domain: "assistant", value: next }); setMessage("语音助手设置已保存"); } catch (error) { setMessage(String(error)); } }
  return <div className="flex flex-col gap-7">
    <SettingsSection title="跨应用语音助手">
      <p className="text-xs text-[var(--color-fg-subtle)]">通过独立快捷键触发。选区只在按键时读取，不会持续监听鼠标或键盘。</p>
      <div className="grid gap-3 md:grid-cols-3">
        {[{ title: "语音翻译", text: "说出内容后翻译并注入原输入框。" }, { title: "选区编辑", text: "先选中文本，再说出缩短、改写或调整语气等指令。" }, { title: "语音问答", text: "可携带选中文本提问，答案显示在独立悬浮窗。" }].map((item) => <div key={item.title} className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4"><strong className="text-sm">{item.title}</strong><p className="mt-2 text-xs leading-5 text-[var(--color-fg-subtle)]">{item.text}</p></div>)}
      </div>
      <p className="text-xs text-[var(--color-fg-subtle)]">三个动作的快捷键在“设置 → 按键”中统一配置。没有可用翻译或大语言模型时，触发前会给出明确配置提示。</p>
    </SettingsSection>
    <SettingsSection title="语音翻译">
      <FormGrid>
        <Field layout="row" label="翻译模型"><Select value={prefs.translationModel} onChange={(event) => void save({ ...prefs, translationModel: event.target.value })}><option value="none">无（暂不启用）</option>{TRANSLATION_MODEL_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}{pluginOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</Select></Field>
        <Field layout="row" label="源语言"><Select value={prefs.sourceLanguage} onChange={(event) => void save({ ...prefs, sourceLanguage: event.target.value })}>{TRANSLATION_SOURCE_LANGUAGE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</Select></Field>
        <Field layout="row" label="目标语言"><Select value={prefs.targetLanguage} onChange={(event) => void save({ ...prefs, targetLanguage: event.target.value })}>{TRANSLATION_TARGET_LANGUAGE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</Select></Field>
      </FormGrid>
      {message && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </SettingsSection>
  </div>;
}
