import { useEffect, useMemo, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { IconButton } from "@/components/ui/IconButton";
import { Input, NumberInput, Select, Textarea } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { useProviderStore } from "@/store/useProviderStore";
import {
  DEFAULT_HOTWORD_WEIGHT,
  HOTWORDS_PLACEHOLDER,
  MAX_CONTEXT_CHARS,
  MAX_HOTWORDS,
  MAX_HOTWORD_WEIGHT,
  MIN_HOTWORD_WEIGHT,
  renderContextPreview,
  supportsHotwordSync,
  useCustomizationStore,
  type Hotword,
  type SyncState,
} from "@/store/useCustomizationStore";
import { useUiStore, type CustomizationTabKey } from "@/store/useUiStore";

const TABS: TabItem<CustomizationTabKey>[] = [
  { key: "hotwords", label: "热词" },
  { key: "context", label: "上下文" },
];

function HotwordsTab() {
  const prefs = useCustomizationStore((state) => state.prefs);
  const patch = useCustomizationStore((state) => state.patch);
  const syncState = useCustomizationStore((state) => state.syncState);
  const syncMessage = useCustomizationStore((state) => state.syncMessage);
  const syncResults = useCustomizationStore((state) => state.syncResults);
  const pullFromProvider = useCustomizationStore((state) => state.pullFromProvider);
  const clearProviders = useCustomizationStore((state) => state.clearProviders);
  const profiles = useProviderStore((state) => state.profiles);

  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [pullProviderId, setPullProviderId] = useState("");
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);

  const targets = useMemo(() => profiles.filter(supportsHotwordSync), [profiles]);

  useEffect(() => {
    if (targets.length && !targets.some((item) => item.id === pullProviderId)) {
      setPullProviderId(targets[0].id);
    }
  }, [targets, pullProviderId]);

  const updateHotword = (index: number, partial: Partial<Hotword>) => {
    const hotwords = prefs.hotwords.map((item, i) => (i === index ? { ...item, ...partial } : item));
    void patch({ hotwords });
  };
  const removeHotword = (index: number) => {
    void patch({ hotwords: prefs.hotwords.filter((_, i) => i !== index) });
  };
  const addHotword = () => {
    if (prefs.hotwords.length >= MAX_HOTWORDS) return;
    void patch({ hotwords: [...prefs.hotwords, { text: "", weight: DEFAULT_HOTWORD_WEIGHT }] });
  };

  const run = async (label: string, action: () => Promise<void>) => {
    setBusy(true);
    setMessage("");
    try {
      await action();
      setMessage(`${label}完成。`);
    } catch (error) {
      setMessage(`${label}失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection
        title="热词表"
        right={<SyncStatus state={syncState} message={syncMessage} />}
      >
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          热词用于提升人名、产品名、专业术语的识别准确率。权重越高，模型越倾向于把相近发音识别成该词；
          不支持权重的供应商会忽略这一列。修改会自动保存，并在停顿后自动同步到需要云端词表的供应商。
        </p>
        <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
          {prefs.hotwords.length === 0 ? (
            <p className="px-3 py-2.5 text-xs text-[var(--color-fg-faint)]">暂无热词</p>
          ) : (
            <div className="max-h-[22rem] overflow-y-auto">
              {prefs.hotwords.map((hotword, index) => (
                <div
                  key={index}
                  className="flex items-center gap-2 border-b border-[var(--color-line)] px-3 py-2 last:border-b-0"
                >
                  <Input
                    value={hotword.text}
                    placeholder="热词，例如 说吧"
                    spellCheck={false}
                    aria-label={`第 ${index + 1} 个热词`}
                    onChange={(event) => updateHotword(index, { text: event.target.value })}
                    size="sm" className="flex-1"
                  />
                  <label className="flex shrink-0 items-center gap-1.5 text-[11px] text-[var(--color-fg-subtle)]">
                    权重
                    <NumberInput
                      value={hotword.weight}
                      min={MIN_HOTWORD_WEIGHT}
                      max={MAX_HOTWORD_WEIGHT}
                      aria-label={`第 ${index + 1} 个热词的权重`}
                      onValueChange={(weight) => updateHotword(index, { weight })}
                      className="w-16"
                      size="sm"
                    />
                  </label>
                  <IconButton
                    size="sm"
                    variant="dangerHover"
                    className="h-7 w-7 shrink-0"
                    label={`删除热词 ${hotword.text || index + 1}`}
                    onClick={() => removeHotword(index)}
                  >
                    <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                  </IconButton>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="flex items-center gap-3">
          <Button size="sm" disabled={prefs.hotwords.length >= MAX_HOTWORDS} onClick={addHotword}>
            <Plus className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
            添加热词
          </Button>
          <span className="text-xs text-[var(--color-fg-faint)]">
            {prefs.hotwords.length} / {MAX_HOTWORDS}
          </span>
        </div>
      </SettingsSection>

      <SettingsSection title="供应商同步">
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          部分供应商要求先把热词表上传到云端再在识别时引用，这一步已自动完成，具体建几份词表、
          绑定哪个模型由「说吧！」处理。只支持上下文的模型无需词表，直接随请求下发。
        </p>
        {targets.length === 0 ? (
          <p className="text-xs text-[var(--color-fg-faint)]">当前没有已启用且支持热词的供应商。</p>
        ) : (
          <Field
            label="云端词表"
            hint="获取会用云端已有的词表覆盖上面的热词列表，上下文模板不受影响。"
            actions={
              <>
                <Button
                  disabled={busy || !pullProviderId}
                  onClick={() => void run("获取热词", () => pullFromProvider(pullProviderId))}
                >
                  获取
                </Button>
                <Button
                  variant="dangerHover"
                  disabled={busy}
                  onClick={() => setClearConfirmOpen(true)}
                >
                  清除
                </Button>
              </>
            }
          >
            <Select
              value={pullProviderId}
              aria-label="云端词表所属的供应商"
              onChange={(event) => setPullProviderId(event.target.value)}
            >
              {targets.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.displayName}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {message && <p className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
        {syncResults.length > 0 && (
          <ul className="flex flex-col gap-1 text-xs">
            {syncResults.map((result) => (
              <li
                key={result.providerId}
                className={result.ok ? "text-[var(--color-fg-subtle)]" : "text-[var(--color-err)]"}
              >
                {result.displayName}：{result.message}
              </li>
            ))}
          </ul>
        )}
      </SettingsSection>

      <Modal
        open={clearConfirmOpen}
        onClose={() => !busy && setClearConfirmOpen(false)}
        title="清除云端词表"
        showCloseButton={false}
        className="max-w-[430px]"
      >
        <div className="p-5">
          <p className="text-sm leading-relaxed text-[var(--color-fg-subtle)]">
            将删除所有已启用供应商在云端保存的热词词表。上面的热词列表会保留，下次修改时会重新上传。
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button size="sm" autoFocus disabled={busy} onClick={() => setClearConfirmOpen(false)}>
              取消
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={busy}
              onClick={async () => {
                await run("清除云端词表", clearProviders);
                setClearConfirmOpen(false);
              }}
            >
              {busy ? "正在清除..." : "清除词表"}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

const SYNC_TONE: Record<SyncState, string> = {
  idle: "",
  pending: "text-[var(--color-fg-faint)]",
  syncing: "text-[var(--color-fg-subtle)]",
  done: "text-[var(--color-fg-subtle)]",
  error: "text-[var(--color-err)]",
};

function SyncStatus({ state, message }: { state: SyncState; message: string }) {
  if (state === "idle" || !message) return null;
  return (
    <span className={`text-xs ${SYNC_TONE[state]}`} role="status">
      {message}
    </span>
  );
}

function ContextTab() {
  const prefs = useCustomizationStore((state) => state.prefs);
  const patch = useCustomizationStore((state) => state.patch);
  const preview = renderContextPreview(prefs);

  const insertHotwordsVariable = () => {
    const template = prefs.contextTemplate;
    void patch({
      contextTemplate: template ? `${template}${HOTWORDS_PLACEHOLDER}` : HOTWORDS_PLACEHOLDER,
    });
  };

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection title="上下文模板">
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          部分模型不接受热词表，而是接受一段上下文文本，靠其中出现的原词来纠正专有名词。
          上下文完全由这里的模板决定：模板留空就不下发上下文；需要带上热词表时，用
          <code className="mx-1 text-[var(--color-accent-light)]">{HOTWORDS_PLACEHOLDER}</code>
          变量引用，不插入变量就不会带热词。
        </p>
        <Field
          label="模板"
          hint="上下文按词表匹配生效，需要包含音频里会出现的原词；只写语义描述不会起作用。"
        >
          <Textarea
            rows={8}
            spellCheck={false}
            placeholder={`例如：\n本次录音涉及以下术语：${HOTWORDS_PLACEHOLDER}。内容为一场关于语音识别的技术分享。`}
            value={prefs.contextTemplate}
            onChange={(event) => void patch({ contextTemplate: event.target.value })}
          />
        </Field>
        <div>
          <Button size="sm" onClick={insertHotwordsVariable}>
            插入热词
          </Button>
        </div>
      </SettingsSection>

      <SettingsSection title="实际下发内容">
        <p className="text-xs text-[var(--color-fg-subtle)]">
          变量已展开，超出 {MAX_CONTEXT_CHARS} 字符的部分会被截断（供应商侧同样限制）。 当前{" "}
          {preview.length} / {MAX_CONTEXT_CHARS} 字符。
        </p>
        <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)] px-3 py-2.5 text-xs text-[var(--color-fg-muted)]">
          {preview || "（空，不会向模型下发上下文）"}
        </pre>
      </SettingsSection>
    </div>
  );
}

export function CustomizationView() {
  const tab = useUiStore((state) => state.customizationTab);
  const setTab = useUiStore((state) => state.setCustomizationTab);

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="热词与上下文"
        description="全局维护一份热词与上下文：支持热词的模型收到词表，支持上下文的模型收到渲染后的文本。"
      />

      <Tabs<CustomizationTabKey>
        id="customization-tabs"
        ariaLabel="热词与上下文分类"
        tabs={TABS}
        active={tab}
        onChange={setTab}
      />

      <div
        id={`customization-tabs-${tab}-panel`}
        role="tabpanel"
        aria-labelledby={`customization-tabs-${tab}-tab`}
      >
        {tab === "hotwords" ? <HotwordsTab /> : <ContextTab />}
      </div>
    </div>
  );
}
