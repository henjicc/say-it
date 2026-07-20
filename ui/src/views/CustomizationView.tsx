import { useEffect, useMemo, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { IconButton } from "@/components/ui/IconButton";
import { Input, Select, Textarea } from "@/components/ui/Input";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { useProviderStore, type ProviderProfile } from "@/store/useProviderStore";
import {
  DEFAULT_HOTWORD_WEIGHT,
  HOTWORDS_PLACEHOLDER,
  MAX_CONTEXT_CHARS,
  MAX_HOTWORD_WEIGHT,
  MIN_HOTWORD_WEIGHT,
  renderContextPreview,
  useCustomizationStore,
  type Hotword,
} from "@/store/useCustomizationStore";
import { useUiStore, type CustomizationTabKey } from "@/store/useUiStore";

const TABS: TabItem<CustomizationTabKey>[] = [
  { key: "hotwords", label: "热词" },
  { key: "context", label: "上下文" },
];

const WEIGHTS = Array.from(
  { length: MAX_HOTWORD_WEIGHT - MIN_HOTWORD_WEIGHT + 1 },
  (_, index) => MIN_HOTWORD_WEIGHT + index,
);

/** 支持热词同步的供应商，判定与设置页一致。 */
function supportsHotwordSync(profile: ProviderProfile): boolean {
  return (
    profile.enabled &&
    (profile.capabilities.includes("customization") ||
      (profile.actions?.includes("manageHotwords") ?? false))
  );
}

function HotwordsTab() {
  const prefs = useCustomizationStore((state) => state.prefs);
  const patch = useCustomizationStore((state) => state.patch);
  const syncResults = useCustomizationStore((state) => state.syncResults);
  const syncToProviders = useCustomizationStore((state) => state.syncToProviders);
  const pullFromProvider = useCustomizationStore((state) => state.pullFromProvider);
  const clearProviders = useCustomizationStore((state) => state.clearProviders);
  const profiles = useProviderStore((state) => state.profiles);

  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [pullProviderId, setPullProviderId] = useState("");

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
    void patch({
      hotwords: [...prefs.hotwords, { text: "", weight: DEFAULT_HOTWORD_WEIGHT }],
    });
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
      <SettingsSection title="热词表">
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          热词用于提升人名、产品名、专业术语的识别准确率。权重越高，模型越倾向于把相近发音识别成该词；
          不支持权重的供应商会忽略这一列。热词同时可以通过 <code>{HOTWORDS_PLACEHOLDER}</code> 引用到上下文里。
        </p>
        <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
          {prefs.hotwords.length === 0 && (
            <p className="px-3 py-2.5 text-xs text-[var(--color-fg-faint)]">暂无热词</p>
          )}
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
                className="min-h-0 h-8 flex-1 px-2.5 py-1 text-xs"
              />
              <Select
                value={String(hotword.weight)}
                aria-label={`第 ${index + 1} 个热词的权重`}
                onChange={(event) => updateHotword(index, { weight: Number(event.target.value) })}
                className="min-h-0 h-8 w-24 shrink-0 px-2.5 py-1 text-xs"
              >
                {WEIGHTS.map((weight) => (
                  <option key={weight} value={String(weight)}>
                    权重 {weight}
                  </option>
                ))}
              </Select>
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
        <div>
          <Button size="sm" onClick={addHotword}>
            <Plus className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
            添加热词
          </Button>
        </div>
      </SettingsSection>

      <SettingsSection title="供应商同步">
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          部分供应商要求先把热词表上传到云端再在识别时引用。点一次同步即可推送到所有已启用且支持热词的供应商，
          具体建几份词表、绑定哪个模型由「说吧！」自动处理。只支持上下文的模型无需同步，直接随请求下发。
        </p>
        {targets.length === 0 ? (
          <p className="text-xs text-[var(--color-fg-faint)]">
            当前没有已启用且支持热词的供应商。
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="primary"
                disabled={busy || prefs.hotwords.length === 0}
                onClick={() => void run("同步到供应商", syncToProviders)}
              >
                同步到供应商
              </Button>
              <Button
                size="sm"
                disabled={busy}
                onClick={() => void run("清除云端词表", clearProviders)}
              >
                清除云端词表
              </Button>
            </div>
            <Field
              label="从供应商获取热词"
              hint="用云端已有的词表覆盖上面的热词列表，上下文模板不受影响。"
              actions={
                <Button
                  size="sm"
                  disabled={busy || !pullProviderId}
                  onClick={() => void run("获取热词", () => pullFromProvider(pullProviderId))}
                >
                  获取
                </Button>
              }
            >
              <Select
                value={pullProviderId}
                aria-label="获取热词的供应商"
                onChange={(event) => setPullProviderId(event.target.value)}
              >
                {targets.map((profile) => (
                  <option key={profile.id} value={profile.id}>
                    {profile.displayName}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
        )}
        {message && <p className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
        {syncResults.length > 0 && (
          <ul className="flex flex-col gap-1 text-xs">
            {syncResults.map((result) => (
              <li
                key={result.providerId}
                className={
                  result.ok ? "text-[var(--color-fg-subtle)]" : "text-[var(--color-danger)]"
                }
              >
                {result.displayName}：{result.message}
              </li>
            ))}
          </ul>
        )}
      </SettingsSection>
    </div>
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
          在这里编写上下文，用 <code>{HOTWORDS_PLACEHOLDER}</code> 引用热词表的内容。
          模板留空时自动使用纯热词列表，因此只填热词也能在这类模型上生效。
        </p>
        <Field
          label="模板"
          hint="上下文按词表匹配生效，需要包含音频里会出现的原词；只写语义描述不会起作用。"
          actions={
            <Button size="sm" onClick={insertHotwordsVariable}>
              插入 {HOTWORDS_PLACEHOLDER}
            </Button>
          }
        >
          <Textarea
            rows={8}
            spellCheck={false}
            placeholder={`例如：\n本次录音涉及以下术语：${HOTWORDS_PLACEHOLDER}。内容为一场关于语音识别的技术分享。`}
            value={prefs.contextTemplate}
            onChange={(event) => void patch({ contextTemplate: event.target.value })}
          />
        </Field>
      </SettingsSection>

      <SettingsSection title="实际下发内容">
        <p className="text-xs text-[var(--color-fg-subtle)]">
          变量已展开，超出 {MAX_CONTEXT_CHARS} 字符的部分会被截断（供应商侧同样限制）。
          当前 {preview.length} / {MAX_CONTEXT_CHARS} 字符。
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
