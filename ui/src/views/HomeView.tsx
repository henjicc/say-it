import { useEffect, useMemo, useState } from "react";
import { CheckCircle2, ChevronRight, CircleAlert, Clock3, Languages, Mic, Sparkles, TextCursorInput, WandSparkles } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Select } from "@/components/ui/Input";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { ShortcutRecorder } from "@/features/dictation/ShortcutRecorder";
import { DICTATION_ASR_MODEL_OPTIONS } from "@/features/asr/modelOptions";
import { useModelCatalogRevision } from "@/features/asr/modelRegistry";
import { loadShortcutBindings, shortcutTargetKey, updateShortcutBinding, type ShortcutBindingItem } from "@/features/hotkeys/catalog";
import { reportShortcutConflict } from "@/features/hotkeys/conflictFeedback";
import { CMD, EVT, cmd, on, type SetupStatus, type UsageSummary } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useProviderStore } from "@/store/useProviderStore";
import { useUiStore } from "@/store/useUiStore";

const actionMeta = {
  dictationMain: { title: "语音输入", description: "把口述内容直接变成清晰文字", icon: Mic },
  translateSpeech: { title: "语音翻译", description: "说出内容，翻译后输入", icon: Languages },
  editSelection: { title: "选区编辑", description: "选中文字，再说出修改要求", icon: WandSparkles },
  ask: { title: "语音问答", description: "携带选区提问或直接问问题", icon: Sparkles },
};

function itemKey(item: ShortcutBindingItem) {
  return item.target.kind === "dictationMain" ? "dictationMain" : item.target.kind === "assistant" ? item.target.action : "";
}
function ensureMainShortcut(items: ShortcutBindingItem[]) {
  return items.some((item) => item.target.kind === "dictationMain") ? items : [{ target: { kind: "dictationMain" } as const, name: "语音输入 · 主快捷键", actionLabel: "开始或结束语音输入", enabled: false, keyCode: "", ctrl: false, shift: false, alt: false, meta: false, triggerMode: "toggle" as const, triggerModeEditable: true }, ...items];
}
function formatDuration(ms: number) {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes} 分钟`;
  return `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分`;
}

export function HomeView() {
  useModelCatalogRevision();
  const [shortcuts, setShortcuts] = useState<ShortcutBindingItem[]>([]);
  const [busy, setBusy] = useState("");
  const [setup, setSetup] = useState<SetupStatus>();
  const [usage, setUsage] = useState<UsageSummary>({ successfulActions: 0, outputChars: 0, spokenDurationMs: 0, estimatedTimeSavedMs: 0 });
  const [message, setMessage] = useState("");
  const asrModel = useDictPrefs((state) => state.prefs.asrModel);
  const patchDict = useDictPrefs((state) => state.patch);
  const profiles = useProviderStore((state) => state.profiles).filter((item) => item.enabled && item.kind.startsWith("llm:"));
  const defaults = useProviderStore((state) => state.defaults);
  const setDefault = useProviderStore((state) => state.setDefault);
  const updateProviderConfig = useProviderStore((state) => state.updateConfig);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);
  const visibleShortcuts = useMemo(() => shortcuts.filter((item) => Boolean(itemKey(item))), [shortcuts]);
  const smartModelOptions = useMemo(() => profiles.flatMap((profile) => {
    const models = Array.isArray(profile.config?.models)
      ? profile.config.models.flatMap((item) => item && typeof item === "object" && typeof (item as { name?: unknown }).name === "string" ? [String((item as { name: string }).name)] : [])
      : [];
    const current = typeof profile.config?.model === "string" ? profile.config.model : "";
    if (current && !models.includes(current)) models.unshift(current);
    return models.map((model) => ({ value: JSON.stringify([profile.id, model]), label: `${profile.displayName} · ${model}` }));
  }), [profiles]);
  const defaultSmartProfile = profiles.find((item) => item.id === defaults.llm);
  const defaultSmartModel = typeof defaultSmartProfile?.config?.model === "string" ? defaultSmartProfile.config.model : "";
  const selectedSmartModel = defaults.llm && defaultSmartModel ? JSON.stringify([defaults.llm, defaultSmartModel]) : "";

  const refreshUsage = () => cmd<UsageSummary>(CMD.getUsageSummary).then(setUsage).catch(() => {});
  useEffect(() => {
    void loadShortcutBindings().then((items) => setShortcuts(ensureMainShortcut(items))).catch((error) => setMessage(String(error)));
    void cmd<SetupStatus>(CMD.getSetupStatus).then(setSetup).catch(() => {});
    void refreshUsage();
    let unlisten: (() => void) | undefined;
    void on(EVT.historyChanged, refreshUsage).then((value) => { unlisten = value; });
    return () => unlisten?.();
  }, []);

  const changeShortcut = async (item: ShortcutBindingItem, next: Parameters<typeof updateShortcutBinding>[1]) => {
    const key = shortcutTargetKey(item.target); setBusy(key); setMessage("");
    try { setShortcuts(ensureMainShortcut(await updateShortcutBinding(item, next, item.triggerMode))); }
    catch (error) { if (!reportShortcutConflict(error)) setMessage(String(error)); setShortcuts(ensureMainShortcut(await loadShortcutBindings().catch(() => shortcuts))); }
    finally { setBusy(""); }
  };
  const openStatus = () => { setSettingsTab("general"); setView("settings"); };
  const changeSmartModel = async (value: string) => {
    if (!value) return;
    try {
      const [providerId, model] = JSON.parse(value) as [string, string];
      const profile = profiles.find((item) => item.id === providerId);
      if (!profile) return;
      await updateProviderConfig(providerId, { ...profile.config, model });
      await setDefault("llm", providerId);
    } catch (error) { setMessage(`切换智能模型失败：${String(error)}`); }
  };
  const blocked = setup?.checks.filter((item) => item.status !== "ready") ?? [];

  return <div className="flex flex-col gap-8">
    <SettingsSection title="快捷操作">
      <div className="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)]">
        {visibleShortcuts.map((item) => {
          const key = itemKey(item) as keyof typeof actionMeta; const meta = actionMeta[key]; const Icon = meta.icon;
          return <div key={shortcutTargetKey(item.target)} className="grid items-center gap-4 border-b border-[var(--color-line)] px-4 py-3 last:border-0 md:grid-cols-[minmax(0,1fr)_minmax(260px,0.72fr)]">
            <div className="flex items-center gap-3"><span className="grid h-9 w-9 place-items-center rounded-[var(--radius-md)] bg-[var(--accent-soft)] text-[var(--color-accent-light)]"><Icon className="h-4.5 w-4.5" /></span><div><p className="text-sm font-medium text-[var(--color-fg)]">{meta.title}</p><p className="mt-0.5 text-xs text-[var(--color-fg-subtle)]">{meta.description}</p></div></div>
            <ShortcutRecorder value={item} disabled={Boolean(busy)} ariaLabel={`${meta.title}快捷键`} onChange={(next) => void changeShortcut(item, next)} />
          </div>;
        })}
      </div>
    </SettingsSection>

    <SettingsSection title="快速设置">
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        <Field label="主语音识别模型"><Select value={asrModel} onChange={(event) => void patchDict({ asrModel: event.target.value })}>{DICTATION_ASR_MODEL_OPTIONS.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</Select></Field>
        <Field label="全局默认智能模型"><Select value={selectedSmartModel} onChange={(event) => void changeSmartModel(event.target.value)}><option value="">尚未配置</option>{smartModelOptions.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</Select></Field>
        <Field label="环境状态" controlId="home-environment-status" className="md:col-span-2 xl:col-span-1">
          <button id="home-environment-status" type="button" onClick={openStatus} className="flex h-[var(--control-h)] w-full items-center gap-3 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-surface)] px-3 text-left transition-colors hover:border-[var(--color-line-strong)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]">
            {setup && blocked.length === 0 ? <CheckCircle2 className="h-4.5 w-4.5 shrink-0 text-[var(--color-ok)]" /> : <CircleAlert className="h-4.5 w-4.5 shrink-0 text-[var(--color-warn)]" />}
            <span className="min-w-0 flex-1 truncate text-sm font-medium text-[var(--color-fg)]">{!setup ? "正在检查环境…" : blocked.length === 0 ? "所有关键能力均可用" : `${blocked.length} 项需要处理`}</span>
            <ChevronRight className="h-4 w-4 shrink-0 text-[var(--color-fg-faint)]" />
          </button>
        </Field>
      </div>
    </SettingsSection>

    <SettingsSection title="本地累计使用">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {[
          { label: "成功操作", value: usage.successfulActions.toLocaleString(), suffix: "次", icon: CheckCircle2 },
          { label: "输出字数", value: usage.outputChars.toLocaleString(), suffix: "字", icon: TextCursorInput },
          { label: "口述时长", value: formatDuration(usage.spokenDurationMs), suffix: "", icon: Mic },
          { label: "估算节省", value: formatDuration(usage.estimatedTimeSavedMs), suffix: "", icon: Clock3 },
        ].map((item) => <div key={item.label} className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4"><item.icon className="h-4 w-4 text-[var(--color-accent-light)]" /><p className="mt-4 text-2xl font-semibold text-[var(--color-fg)]">{item.value}<span className="ml-1 text-xs font-normal text-[var(--color-fg-subtle)]">{item.suffix}</span></p><p className="mt-1 text-xs text-[var(--color-fg-subtle)]">{item.label}</p></div>)}
      </div>
      <p className="text-xs text-[var(--color-fg-faint)]">仅保存聚合数字，不保存正文或音频。节省时间按中文 40 字/分钟、英文 40 词/分钟估算。</p>
    </SettingsSection>
    {message && <p role="status" className="text-xs text-[var(--color-err)]">{message}</p>}
  </div>;
}
