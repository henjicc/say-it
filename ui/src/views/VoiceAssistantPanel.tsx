import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, CircleHelp, Languages, WandSparkles } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { Select, Textarea } from "@/components/ui/Input";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Switch } from "@/components/ui/Switch";
import { optionsForScene, useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { TRANSLATION_MODEL_OPTIONS } from "@/features/translation/models";
import {
  TRANSLATION_SOURCE_LANGUAGE_OPTIONS,
  TRANSLATION_TARGET_LANGUAGE_OPTIONS,
} from "@/features/translation/languages";
import { cn } from "@/lib/cn";
import { CMD, cmd, type AppSnapshot } from "@/lib/tauri";
import { useProviderStore, type ProviderProfile } from "@/store/useProviderStore";
import { useUiStore, type AssistantActionKey } from "@/store/useUiStore";

interface AssistantPrefs {
  translationModel: string;
  sourceLanguage: string;
  targetLanguage: string;
  llmProviderId: string;
  llmModel: string;
  preserveStructure: boolean;
  answerStyle: "concise" | "balanced" | "detailed";
  customInstructions: string;
}

const DEFAULT_PREFS: AssistantPrefs = {
  translationModel: "none",
  sourceLanguage: "auto",
  targetLanguage: "zh",
  llmProviderId: "default",
  llmModel: "",
  preserveStructure: true,
  answerStyle: "balanced",
  customInstructions: "",
};

const ACTIONS: Array<{
  action: AssistantActionKey;
  title: string;
  description: string;
  details: string;
  icon: React.ReactNode;
}> = [
  {
    action: "translateSpeech",
    title: "语音翻译",
    description: "说出内容，翻译后直接注入当前输入框。",
    details: "使用专用翻译模型，适合低延迟的直接口述翻译。",
    icon: <Languages className="h-5 w-5" aria-hidden />,
  },
  {
    action: "editSelection",
    title: "选区编辑",
    description: "选中文本后，说出翻译、优化、邮件化或改写指令。",
    details: "大语言模型会先识别语音意图，再按内置质量规则处理；选区变化时不会覆盖原文。",
    icon: <WandSparkles className="h-5 w-5" aria-hidden />,
  },
  {
    action: "ask",
    title: "语音问答",
    description: "针对选中文本提问，也可以直接提出一般问题。",
    details: "答案显示在独立悬浮窗，不会自动修改原文。",
    icon: <CircleHelp className="h-5 w-5" aria-hidden />,
  },
];

function normalizePrefs(value: Record<string, unknown>): AssistantPrefs {
  const answerStyle = value.answerStyle;
  return {
    translationModel: typeof value.translationModel === "string" ? value.translationModel : DEFAULT_PREFS.translationModel,
    sourceLanguage: typeof value.sourceLanguage === "string" ? value.sourceLanguage : DEFAULT_PREFS.sourceLanguage,
    targetLanguage: typeof value.targetLanguage === "string" ? value.targetLanguage : DEFAULT_PREFS.targetLanguage,
    llmProviderId: typeof value.llmProviderId === "string" ? value.llmProviderId : DEFAULT_PREFS.llmProviderId,
    llmModel: typeof value.llmModel === "string" ? value.llmModel : DEFAULT_PREFS.llmModel,
    preserveStructure: value.preserveStructure !== false,
    answerStyle: answerStyle === "concise" || answerStyle === "detailed" ? answerStyle : "balanced",
    customInstructions: typeof value.customInstructions === "string" ? value.customInstructions : "",
  };
}

function modelsFromProfile(profile: ProviderProfile): string[] {
  const configured = profile.config?.models;
  const models = Array.isArray(configured)
    ? configured.flatMap((item) => {
      if (!item || typeof item !== "object") return [];
      const name = (item as { name?: unknown }).name;
      return typeof name === "string" && name.trim() ? [name.trim()] : [];
    })
    : [];
  const current = profile.config?.model;
  if (typeof current === "string" && current.trim() && !models.includes(current.trim())) {
    models.unshift(current.trim());
  }
  return models;
}

function modelChoiceValue(providerId: string, model: string) {
  return JSON.stringify([providerId, model]);
}

function parseModelChoice(value: string): Pick<AssistantPrefs, "llmProviderId" | "llmModel"> {
  if (value === "default") return { llmProviderId: "default", llmModel: "" };
  try {
    const parsed = JSON.parse(value) as unknown;
    if (Array.isArray(parsed) && parsed.length === 2 && parsed.every((item) => typeof item === "string")) {
      return { llmProviderId: parsed[0], llmModel: parsed[1] };
    }
  } catch {
    // Select 的值只由本组件生成；异常时安全回退到全局默认。
  }
  return { llmProviderId: "default", llmModel: "" };
}

export function VoiceAssistantView() {
  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="语音助手"
        description="用专用快捷键完成跨应用翻译、选区编辑和语音问答。选区只在触发时读取。"
      />
      <VoiceAssistantPanel />
    </div>
  );
}

export function VoiceAssistantPanel() {
  useModelCatalogRevision();
  const [prefs, setPrefs] = useState(DEFAULT_PREFS);
  const [message, setMessage] = useState("");
  const [messageError, setMessageError] = useState(false);
  const [previewAction, setPreviewAction] = useState<AssistantActionKey>("editSelection");
  const [previewSelection, setPreviewSelection] = useState("明天下午把新版方案发给客户，内容要写清楚，但是不要承诺具体上线日期。");
  const [previewInstruction, setPreviewInstruction] = useState("改成一封简洁、专业的邮件");
  const [previewResult, setPreviewResult] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const focusedAction = useUiStore((state) => state.focusedAssistantAction);
  const consumeFocusedAction = useUiStore((state) => state.consumeFocusedAssistantAction);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);
  const [highlightedAction, setHighlightedAction] = useState<AssistantActionKey | null>(null);
  const highlightTimer = useRef<number | undefined>(undefined);
  const profiles = useProviderStore((state) => state.profiles).filter(
    (profile) => profile.enabled && profile.kind.startsWith("llm:"),
  );
  const providerDefaults = useProviderStore((state) => state.defaults);
  const pluginOptions = optionsForScene("subtitleTranslation");

  const modelOptions = useMemo(() => profiles.flatMap((profile) =>
    modelsFromProfile(profile).map((model) => ({
      value: modelChoiceValue(profile.id, model),
      label: `${profile.displayName} · ${model}`,
    }))), [profiles]);

  const selectedModelValue = prefs.llmProviderId === "default"
    ? "default"
    : modelChoiceValue(prefs.llmProviderId, prefs.llmModel);
  const defaultProfile = profiles.find((profile) => profile.id === providerDefaults.llm);
  const defaultModel = typeof defaultProfile?.config?.model === "string" ? defaultProfile.config.model : "未选择模型";

  useEffect(() => {
    void cmd<AppSnapshot>(CMD.getAppSnapshot)
      .then((snapshot) => setPrefs(normalizePrefs(snapshot.settings.assistantPrefs)))
      .catch((error) => {
        setMessage(`读取设置失败：${String(error)}`);
        setMessageError(true);
      });
  }, []);

  useEffect(() => {
    if (!focusedAction) return;
    setHighlightedAction(focusedAction);
    window.requestAnimationFrame(() => {
      document.getElementById(`assistant-action-${focusedAction}`)?.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    });
    consumeFocusedAction();
    window.clearTimeout(highlightTimer.current);
    highlightTimer.current = window.setTimeout(() => setHighlightedAction(null), 2400);
    return () => window.clearTimeout(highlightTimer.current);
  }, [consumeFocusedAction, focusedAction]);

  async function save(next: AssistantPrefs) {
    setPrefs(next);
    setMessageError(false);
    try {
      await cmd(CMD.updateAppSettings, { domain: "assistant", value: next });
      setMessage("语音助手设置已保存");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
      setMessageError(true);
    }
  }

  function openSettings(tab: "model" | "keys") {
    setSettingsTab(tab);
    setView("settings");
  }

  async function runPreview() {
    setPreviewing(true);
    setPreviewResult("");
    setMessage("");
    try {
      await cmd(CMD.updateAppSettings, { domain: "assistant", value: prefs });
      const result = await cmd<string>(CMD.previewAssistant, {
        action: previewAction,
        selectedText: previewSelection,
        spokenText: previewInstruction,
      });
      setPreviewResult(result);
      setMessage("试运行完成；结果只显示在这里，不会写入其他应用或历史。 ");
      setMessageError(false);
    } catch (error) {
      setMessage(`试运行失败：${String(error)}`);
      setMessageError(true);
    } finally {
      setPreviewing(false);
    }
  }

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection
        title="能力与快捷键"
        right={<Button size="sm" onClick={() => openSettings("keys")}>配置快捷键<ArrowRight className="h-3.5 w-3.5" aria-hidden /></Button>}
      >
        <div className="grid gap-3 md:grid-cols-3">
          {ACTIONS.map((item) => (
            <article
              id={`assistant-action-${item.action}`}
              key={item.action}
              tabIndex={-1}
              className={cn(
                "rounded-[var(--radius-lg)] border bg-[var(--color-surface)] p-4 outline-none transition-colors duration-[var(--dur-normal)]",
                highlightedAction === item.action
                  ? "border-[var(--color-accent)] bg-[var(--accent-soft)] ring-2 ring-[var(--accent-ring)]"
                  : "border-[var(--color-line)]",
              )}
            >
              <div className="flex items-center gap-2 text-[var(--color-accent-light)]">
                {item.icon}
                <h2 className="text-sm font-semibold text-[var(--color-fg)]">{item.title}</h2>
              </div>
              <p className="mt-3 text-xs leading-5 text-[var(--color-fg-muted)]">{item.description}</p>
              <p className="mt-2 text-xs leading-5 text-[var(--color-fg-subtle)]">{item.details}</p>
            </article>
          ))}
        </div>
      </SettingsSection>

      <SettingsSection
        title="大语言模型"
        right={<Button size="sm" onClick={() => openSettings("model")}>管理模型<ArrowRight className="h-3.5 w-3.5" aria-hidden /></Button>}
      >
        <p className="max-w-[78ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          选区编辑和语音问答使用这里的模型。内置角色会区分选中文本与语音指令，并对翻译、改写和邮件格式执行固定质量规则。
        </p>
        <FormGrid>
          <Field label="处理模型" hint={`跟随默认时当前使用：${defaultProfile?.displayName ?? "未配置"} · ${defaultModel}`}>
            <Select
              value={selectedModelValue}
              onChange={(event) => void save({ ...prefs, ...parseModelChoice(event.target.value) })}
              aria-label="语音助手处理模型"
            >
              <option value="default">跟随全局默认模型</option>
              {modelOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </Select>
          </Field>
          <Field label="问答详细程度" hint="只影响语音问答；明确的口述要求始终优先。">
            <Select
              value={prefs.answerStyle}
              onChange={(event) => void save({ ...prefs, answerStyle: event.target.value as AssistantPrefs["answerStyle"] })}
              aria-label="问答详细程度"
            >
              <option value="concise">简洁</option>
              <option value="balanced">平衡</option>
              <option value="detailed">详细</option>
            </Select>
          </Field>
          <Field label="保留原文结构" hint="除非语音明确要求重排，否则保留段落、列表和换行。">
            <Switch
              checked={prefs.preserveStructure}
              onChange={(preserveStructure) => void save({ ...prefs, preserveStructure })}
              label="保留原文结构"
            />
          </Field>
          <Field
            label="长期偏好"
            hint={`${prefs.customInstructions.length}/4000。作为内置规则之上的个人偏好，不会覆盖事实与安全约束。`}
            className="sm:col-span-2"
          >
            <Textarea
              value={prefs.customInstructions}
              maxLength={4000}
              rows={4}
              placeholder="例如：邮件保持克制、避免感叹号；技术名词保留英文。"
              onChange={(event) => setPrefs({ ...prefs, customInstructions: event.target.value })}
              onBlur={(event) => void save({ ...prefs, customInstructions: event.currentTarget.value })}
              aria-label="语音助手长期偏好"
            />
          </Field>
        </FormGrid>
      </SettingsSection>

      <SettingsSection title="语音翻译">
        <p className="max-w-[78ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          这个动作只翻译口述内容并注入，使用低延迟的专用翻译服务。若要翻译已经选中的文字，请使用“选区编辑”并说出目标语言。
        </p>
        <FormGrid>
          <Field label="翻译模型">
            <Select value={prefs.translationModel} onChange={(event) => void save({ ...prefs, translationModel: event.target.value })}>
              <option value="none">无（暂不启用）</option>
              {TRANSLATION_MODEL_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
              {pluginOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </Select>
          </Field>
          <Field label="源语言">
            <Select value={prefs.sourceLanguage} onChange={(event) => void save({ ...prefs, sourceLanguage: event.target.value })}>
              {TRANSLATION_SOURCE_LANGUAGE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </Select>
          </Field>
          <Field label="目标语言">
            <Select value={prefs.targetLanguage} onChange={(event) => void save({ ...prefs, targetLanguage: event.target.value })}>
              {TRANSLATION_TARGET_LANGUAGE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </Select>
          </Field>
        </FormGrid>
      </SettingsSection>

      <SettingsSection title="试运行">
        <p className="max-w-[78ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          直接调用当前配置的真实模型，便于检查意图识别和输出质量；不会录音、注入、读取真实选区或写入历史。
        </p>
        <FormGrid>
          <Field label="测试动作">
            <Select value={previewAction} onChange={(event) => setPreviewAction(event.target.value as AssistantActionKey)}>
              <option value="editSelection">选区编辑</option>
              <option value="ask">语音问答</option>
              <option value="translateSpeech">语音翻译</option>
            </Select>
          </Field>
          {previewAction !== "translateSpeech" && (
            <Field label="模拟选中文本" hint={previewAction === "ask" ? "可留空，以测试没有选区的一般问答。" : undefined} className="sm:col-span-2">
              <Textarea rows={5} value={previewSelection} onChange={(event) => setPreviewSelection(event.target.value)} />
            </Field>
          )}
          <Field label={previewAction === "translateSpeech" ? "模拟口述内容" : "模拟语音指令"} className="sm:col-span-2">
            <Textarea rows={3} value={previewInstruction} onChange={(event) => setPreviewInstruction(event.target.value)} />
          </Field>
        </FormGrid>
        <div className="flex items-center gap-3">
          <Button variant="primary" disabled={previewing || !previewInstruction.trim()} onClick={() => void runPreview()}>
            {previewing ? "正在调用模型…" : "运行测试"}
          </Button>
          <span className="text-xs text-[var(--color-fg-subtle)]">真实请求可能产生供应商用量。</span>
        </div>
        {previewResult && (
          <Field label="模型结果">
            <Textarea rows={7} readOnly value={previewResult} aria-label="语音助手试运行结果" />
          </Field>
        )}
        {message && (
          <p role={messageError ? "alert" : "status"} className={cn("text-xs", messageError ? "text-[var(--color-err)]" : "text-[var(--color-fg-subtle)]")}>
            {message}
          </p>
        )}
      </SettingsSection>
    </div>
  );
}
