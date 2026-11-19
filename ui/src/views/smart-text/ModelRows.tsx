import { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Row, ROW_CONTROL_W } from "@/components/ui/Group";
import { Select } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { Modal } from "@/components/ui/Modal";
import { ocrOptionsForScene, useModelCatalogRevision, type OcrModelOption } from "@/features/asr/modelRegistry";
import { ModelPicker, type ModelPickerOption } from "@/features/models/ModelPicker";
import { LOCAL_POLISH_TEMPLATES } from "@/features/onboarding/setup";
import { FOLLOW_DEFAULT_LLM_OPTION, llmModelPickerOptions, llmModelValue, localPolishModelOptions } from "@/features/models/llmModelOptions";
import { useUsageIssue } from "@/features/models/usage";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";

/** 听写智能处理用的大模型。label 是「用途」页里按功能名显示的行名。 */
export function SmartLlmRow({ label = "智能模型" }: { label?: string } = {}) {
  const prefs = useDictPrefs((state) => state.prefs);
  const patch = useDictPrefs((state) => state.patch);
  const allProfiles = useProviderStore((state) => state.profiles);
  const profiles = allProfiles.filter((profile) => profile.enabled && profile.capabilities.includes("llm"));
  const localOptions = localPolishModelOptions(allProfiles);
  const selected = prefs.smartLlmProviderId === "default" ? "default" : llmModelValue(prefs.smartLlmProviderId, prefs.smartLlmModel);
  const options: ModelPickerOption[] = [FOLLOW_DEFAULT_LLM_OPTION, ...localOptions, ...llmModelPickerOptions(profiles, selected)];
  const local = localOptions.some((option) => option.value === selected);
  const usageIssue = useUsageIssue(prefs.smartLlmProviderId === "default" ? undefined : prefs.smartLlmProviderId, true, "llm");
  const issue = local && !LOCAL_POLISH_TEMPLATES.includes(prefs.smartTemplateId)
    ? <span role="status" className="text-[var(--color-err)]">本地整理模型只支持「场景感知润色」和「通用润色」模板，当前模板请换用大模型。</span>
    : usageIssue;
  const current = options.find((option) => option.value === selected);
  return (
    <>
      <Row label={label} controlId="smart-text-llm-model" message={issue}>
        <div className={ROW_CONTROL_W}>
          <ModelPicker
            id="smart-text-llm-model"
            value={selected}
            options={options}
            panelLabel="选择智能模型"
            onChange={(value) => {
              if (value === "default") {
                void patch({ smartLlmProviderId: "default", smartLlmModel: "" });
                return;
              }
              const [providerId, model] = JSON.parse(value) as [string, string];
              void patch({ smartLlmProviderId: providerId, smartLlmModel: model });
            }}
          />
        </div>
      </Row>
    </>
  );
}

/** 本地整理模型自动识别意图的开关；智能模型不是本地整理模型时不显示。 */
export function AutoIntentRow() {
  const prefs = useDictPrefs((state) => state.prefs);
  const patch = useDictPrefs((state) => state.patch);
  const local = useProviderStore((state) => state.profiles).some(
    (profile) => profile.enabled && profile.id === prefs.smartLlmProviderId && profile.capabilities.includes("polish"),
  );
  if (!local || !LOCAL_POLISH_TEMPLATES.includes(prefs.smartTemplateId)) return null;
  return (
    <Row
      label="自动识别意图"
      controlId="smart-text-auto-intent"
      hint="开启后，语音输入快捷键会按你说的话自动判断是听写、改写选中文字、翻译、查词还是提问。关闭时只做听写，改写、翻译和提问用「助手」里各自的快捷键。"
    >
      <Switch id="smart-text-auto-intent" checked={prefs.smartAutoIntent} onChange={(checked) => void patch({ smartAutoIntent: checked })} />
    </Row>
  );
}

/** 听写读取当前窗口文字时用的 OCR 模型；第三方 OCR 首次选择需要确认会发送截图。label 是「用途」页里按功能名显示的行名。 */
export function OcrModelRow({ label = "OCR 模型" }: { label?: string } = {}) {
  useModelCatalogRevision();
  const prefs = useDictPrefs((state) => state.prefs);
  const patch = useDictPrefs((state) => state.patch);
  const [message, setMessage] = useState("");
  const [pending, setPending] = useState<OcrModelOption>();
  const models = ocrOptionsForScene("activeAppContext");
  const selected = models.find((option) => option.value === prefs.activeAppContextOcrModel);

  const select = async (model: OcrModelOption) => {
    setMessage("");
    if (model.remote && !prefs.activeAppContextOcrApprovedProviders.includes(model.providerId)) {
      setPending(model);
      return;
    }
    try {
      await patch({
        activeAppContextOcrModel: model.value,
        activeAppContextOcrEngine: model.value === "local-ppocr-v6-tiny" ? "ppocr" : "system",
      });
    } catch (error) {
      setMessage(`OCR 模型切换失败：${String(error)}`);
    }
  };

  const approve = async () => {
    if (!pending) return;
    setMessage("");
    try {
      await patch({
        activeAppContextOcrModel: pending.value,
        activeAppContextOcrEngine: "system",
        activeAppContextOcrApprovedProviders: Array.from(new Set([
          ...prefs.activeAppContextOcrApprovedProviders,
          pending.providerId,
        ])),
      });
      setPending(undefined);
    } catch (error) {
      setMessage(`OCR 模型切换失败：${String(error)}`);
    }
  };

  return (
    <>
      <Row
        label={label}
        hint={selected?.remote
          ? "场景感知截图会发送给该第三方 OCR 供应商；首次选择需要确认。"
          : selected?.value === "local-ppocr-v6-tiny"
            ? "使用内置 PP-OCRv6 Tiny 本地模型，模型仅在识别期间加载。"
            : "使用系统自带的文字识别，不会向第三方发送截图。"}
        message={message && <span role="alert" className="text-[var(--color-err)]">{message}</span>}
      >
        <Select
          className={ROW_CONTROL_W}
          aria-label={label}
          value={prefs.activeAppContextOcrModel}
          onChange={(event) => {
            const model = models.find((option) => option.value === event.target.value);
            if (model) void select(model);
          }}
        >
          {!selected && <option value={prefs.activeAppContextOcrModel} disabled>当前模型不可用，请重新选择</option>}
          {models.map((model) => <option key={model.value} value={model.value}>{model.label}</option>)}
        </Select>
      </Row>
      <Modal
        open={Boolean(pending)}
        onClose={() => setPending(undefined)}
        title="确认使用第三方 OCR"
        showCloseButton={false}
        className="max-w-[460px]"
      >
        <div className="p-5">
          <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            选择“{pending?.label}”后，场景感知会把当前窗口截图发送给该第三方供应商进行识别。确认后才会保存选择并发送截图。
          </p>
          {message && <p role="alert" className="mt-3 text-xs text-[var(--color-err)]">{message}</p>}
          <div className="mt-6 flex justify-end gap-2">
            <Button size="sm" autoFocus onClick={() => setPending(undefined)}>取消</Button>
            <Button size="sm" variant="primary" onClick={() => void approve()}>确认并使用</Button>
          </div>
        </div>
      </Modal>
    </>
  );
}
