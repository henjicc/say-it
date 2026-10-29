import { HelpLabel } from "@/components/ui/Tooltip";
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
  type CustomizationPrefs,
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

  // 全部基于「写入时的最新状态」计算，并把失败显示出来。
  //
  // 此前这三个操作都用渲染时捕获的 prefs.hotwords 按下标算负载，再 `void patch(...)`
  // 发出去：第一次保存还没返回就做第二次操作，后者会基于旧数组算出负载把前者整份
  // 覆盖掉；而后端校验（单条 64 字符、模板 4000 字符）拒绝时，浮动的 promise 把错误
  // 吞掉，store 不更新，受控输入框无声回滚，用户完全不知道发生了什么。
  const save = (
    label: string,
    updater: (current: CustomizationPrefs) => Partial<CustomizationPrefs>,
  ) => {
    setMessage("");
    void patch(updater).catch((error) => setMessage(`${label}失败：${String(error)}`));
  };
  const updateHotword = (index: number, partial: Partial<Hotword>) => {
    save("保存热词", (current) => ({
      hotwords: current.hotwords.map((item, i) => (i === index ? { ...item, ...partial } : item)),
    }));
  };
  const removeHotword = (index: number) => {
    save("删除热词", (current) => ({
      hotwords: current.hotwords.filter((_, i) => i !== index),
    }));
  };
  const addHotword = () => {
    if (prefs.hotwords.length >= MAX_HOTWORDS) return;
    save("新增热词", (current) => ({
      hotwords: [...current.hotwords, { text: "", weight: DEFAULT_HOTWORD_WEIGHT }],
    }));
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
        description="添加容易听错的人名、产品名或专业词，帮助提高识别准确率。修改后会自动保存并同步。">
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
                    <HelpLabel content="数值越高，发音相近时越优先识别成这个词。部分识别服务不支持此设置。">权重</HelpLabel>
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

      <SettingsSection title="供应商同步" description="需要云端热词表的服务会自动同步，你无需手动上传。">
        {targets.length === 0 ? (
          <p className="text-xs text-[var(--color-fg-faint)]">当前没有已启用且支持热词的供应商。</p>
        ) : (
          <Field
            label="云端词表"
            message="获取会用云端词表替换当前热词列表，上下文模板不受影响。"
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
      <SettingsSection title="上下文模板" description="在这里写下录音中可能出现的人名、术语和背景信息，帮助支持此功能的模型识别。留空则不使用；点击“插入热词”可带上热词表。">
        <Field
          label="模板"
          hint="请写出录音中会出现的具体人名或术语，只有笼统描述无法帮助纠正这些词。"
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

      <SettingsSection title="发送内容预览" description={`这里是识别时会使用的背景文字，最多保留 ${MAX_CONTEXT_CHARS} 个字符。`}>
        <span className="text-xs text-[var(--color-fg-subtle)]">{preview.length} / {MAX_CONTEXT_CHARS} 字符</span>
        <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)] px-3 py-2.5 text-xs text-[var(--color-fg-muted)]">
          {preview || "（未填写，识别时不使用背景文字）"}
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
        description="添加常用词和录音背景，帮助支持此功能的模型更准确地识别。"
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
