import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Brain, Check, Clipboard, Globe2, Pencil, RefreshCw, Search, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Input, Select, Textarea } from "@/components/ui/Input";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, EVT, cmd, type HistoryEntry, type HistoryPage, type LearningOverview, type LearningRule } from "@/lib/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useUiStore } from "@/store/useUiStore";
import { DEFAULT_HOTWORD_WEIGHT, MAX_HOTWORDS, useCustomizationStore } from "@/store/useCustomizationStore";

const PAGE_SIZE = 30;

function taskLabel(kind: HistoryEntry["taskKind"]) {
  return ({ dictation: "听写", translateSpeech: "语音翻译", editSelection: "选区编辑", ask: "语音问答" } as const)[kind] || kind;
}

function statusLabel(status: HistoryEntry["status"]) {
  return ({ recognized: "原文已保存", processed: "结果已保存，待输入", succeeded: "已完成", failed: "失败", cancelled: "已取消" } as const)[status] || status;
}

function finalTextLabel(entry: HistoryEntry) {
  if (entry.learningStatus === "active") return "已学习";
  if (entry.learningStatus === "candidate") return "候选规则";
  if (entry.learningStatus === "pending") return entry.finalTextConfidence === "medium" ? "待确认" : "已捕获";
  if (entry.learningStatus === "rejected") return "不参与学习";
  if (entry.finalTextConfidence === "confirmed") return "已确认";
  if (entry.finalTextConfidence === "high") return "已捕获";
  if (entry.finalTextConfidence === "medium") return "待确认";
  return "";
}

function finalTextSourceLabel(source: HistoryEntry["finalTextSource"]) {
  return ({ keyboard: "回车发送", click: "点击发送", autoEnter: "自动回车", manual: "手工确认" } as const)[source || "keyboard"];
}

function primaryText(entry: HistoryEntry) {
  return entry.finalText || entry.outputText || entry.sourceText;
}

export function HistoryView() {
  const [page, setPage] = useState<HistoryPage>({ items: [], total: 0, recoveryNotice: null });
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState("");
  const [taskKind, setTaskKind] = useState("");
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [overview, setOverview] = useState<LearningOverview>({
    observationEnabled: false,
    learningEnabled: false,
    cloudContextEnabled: false,
    pendingCount: 0,
    activeRuleCount: 0,
    eligibleSampleCount: 0,
    eligibleEntryCount: 0,
    summaryAvailable: false,
    structuredStatistics: {},
  });
  const [rules, setRules] = useState<LearningRule[]>([]);
  const loadVersion = useRef(0);
  const setView = useUiStore((state) => state.setView);
  const setSettingsTab = useUiStore((state) => state.setSettingsTab);
  const hotwords = useCustomizationStore((state) => state.prefs.hotwords);
  const patchCustomization = useCustomizationStore((state) => state.patch);

  const load = useCallback(async () => {
    const version = ++loadVersion.current;
    setLoading(true);
    try {
      const [next, nextOverview, nextRules] = await Promise.all([
        cmd<HistoryPage>(CMD.queryHistory, { query: { search, status, taskKind, offset, limit: PAGE_SIZE } }),
        cmd<LearningOverview>(CMD.getLearningOverview),
        cmd<LearningRule[]>(CMD.queryLearningRules, { query: { search: "", status: "" } }),
      ]);
      if (version === loadVersion.current) {
        setPage(next);
        setOverview(nextOverview);
        setRules(nextRules);
      }
    } catch (error) {
      if (version === loadVersion.current) setMessage(String(error));
    } finally {
      if (version === loadVersion.current) setLoading(false);
    }
  }, [offset, search, status, taskKind]);

  useEffect(() => { void load(); }, [load]);
  useTauriEvent(EVT.historyChanged, () => void load());
  useEffect(() => setOffset(0), [search, status, taskKind]);

  const range = useMemo(() => {
    if (!page.total) return "暂无记录";
    return `${offset + 1}–${Math.min(offset + PAGE_SIZE, page.total)} / ${page.total}`;
  }, [offset, page.total]);

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setMessage("已复制到剪贴板");
    } catch (error) {
      setMessage(`复制失败：${String(error)}`);
    }
  }

  async function save(entry: HistoryEntry) {
    try {
      await cmd(CMD.confirmHistoryFinalText, { id: entry.id, finalText: draft });
      setEditing(null);
      setMessage("已保存，并记录为一条显式纠错样本");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function confirmObserved(entry: HistoryEntry) {
    if (!entry.finalText) return;
    try {
      await cmd(CMD.confirmHistoryFinalText, { id: entry.id, finalText: entry.finalText });
      setMessage("已确认最终草稿并加入学习样本");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function discardObserved(entry: HistoryEntry) {
    try {
      await cmd(CMD.discardHistoryFinalText, { id: entry.id });
      setMessage("已忽略本次观察结果");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function confirmLearning(entry: HistoryEntry) {
    try {
      await cmd(CMD.confirmHistoryLearning, { id: entry.id, scope: "app" });
      setMessage("已确认为应用内学习规则");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function rejectLearning(entry: HistoryEntry) {
    try {
      await cmd(CMD.rejectHistoryLearning, { id: entry.id });
      setMessage("最终草稿已保留，本次修改不会参与学习");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function changeRuleScope(rule: LearningRule) {
    try {
      const scope = rule.scope === "global" ? "app" : "global";
      await cmd(CMD.setLearningRuleScope, { id: rule.id, scope });
      setMessage(scope === "global" ? "规则已改为全局生效" : "规则已限制在来源应用");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function removeRule(rule: LearningRule) {
    if (!window.confirm(`确定删除学习规则“${rule.beforeText} → ${rule.afterText}”吗？`)) return;
    try {
      await cmd(CMD.deleteLearningRule, { id: rule.id });
      setMessage("学习规则已删除");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function toggleRule(rule: LearningRule) {
    try {
      const enabled = rule.status === "disabled";
      await cmd(CMD.setLearningRuleEnabled, { id: rule.id, enabled });
      setMessage(enabled ? "学习规则已重新启用" : "学习规则已停用");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function acceptHotwordSuggestion(rule: LearningRule) {
    const text = rule.afterText.trim();
    if (!text || hotwords.some((item) => item.text.trim().toLocaleLowerCase() === text.toLocaleLowerCase())) {
      setMessage("该词已经在热词表中");
      return;
    }
    if (hotwords.length >= MAX_HOTWORDS) {
      setMessage("热词数量已达到上限");
      return;
    }
    try {
      await patchCustomization({ hotwords: [...hotwords, { text, weight: DEFAULT_HOTWORD_WEIGHT }] });
      setMessage(`已将“${text}”加入热词表`);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function generateSummary() {
    if (!window.confirm("将向当前默认大语言模型发送最多 30 条脱敏的局部修改样本，用于生成表达偏好草稿。是否继续？")) return;
    try {
      setMessage("正在生成表达偏好草稿…");
      await cmd(CMD.generatePreferenceSummary, { scope: "global", providerId: "default", allowCloud: true });
      setMessage("表达偏好草稿已生成，请确认后生效");
      await load();
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function confirmSummary() {
    if (!overview.draftProfile) return;
    try {
      await cmd(CMD.confirmPreferenceSummary, { id: overview.draftProfile.id });
      setMessage("表达偏好已确认并生效");
    } catch (error) {
      setMessage(String(error));
    }
  }

  function openLearningSettings() {
    setSettingsTab("general");
    setView("settings");
  }

  async function remove(id: string) {
    if (!window.confirm("确定删除这条本地历史吗？")) return;
    try {
      await cmd(CMD.deleteHistoryEntry, { id });
      setMessage("记录已删除");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function retry(id: string) {
    try {
      setMessage("正在返回原应用并重试注入…");
      await cmd(CMD.retryHistoryInjection, { id });
      setMessage("已重新注入");
    } catch (error) {
      setMessage(String(error));
    }
  }

  return (
    <div className="flex flex-col gap-7">
      <PageHeader
        title="历史"
        description="默认保留最近 30 天的本地听写与智能助手结果，不保存音频。"
        actions={<Button size="sm" onClick={openLearningSettings}>学习设置</Button>}
      />
      {page.recoveryNotice && (
        <p role="alert" className="rounded-[var(--radius-lg)] border border-[var(--color-warn)]/40 bg-[var(--color-warn)]/10 px-4 py-3 text-sm text-[var(--color-fg)]">
          {page.recoveryNotice}
        </p>
      )}
      <SettingsSection title="个性化学习">
        <div className="flex flex-wrap items-center gap-2 text-sm text-[var(--color-fg-subtle)]">
          <Brain className="h-4 w-4" aria-hidden />
          <span>发送前修改记录：{overview.observationEnabled ? "已开启" : "未开启"}</span>
          <span>·</span>
          <span>个性化纠错：{overview.learningEnabled ? "已开启" : "未开启"}</span>
          <span>·</span>
          <span>{overview.activeRuleCount} 条生效规则</span>
          <span>·</span>
          <span>{overview.pendingCount} 条待处理</span>
          <span>·</span>
          <span>{overview.eligibleSampleCount} 条有效证据 / {overview.eligibleEntryCount} 次听写</span>
        </div>
        {overview.activeProfile && <p className="text-sm text-[var(--color-fg)]">当前表达偏好：{overview.activeProfile.summaryText}</p>}
        {overview.draftProfile && (
          <div className="flex flex-wrap items-center gap-3 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)] p-3 text-sm">
            <span className="min-w-0 flex-1">待确认偏好：{overview.draftProfile.summaryText}</span>
            <Button size="sm" variant="primary" onClick={() => void confirmSummary()}>确认生效</Button>
          </div>
        )}
        <div className="flex flex-wrap gap-2">
          <Button size="sm" disabled={!overview.summaryAvailable} onClick={() => void generateSummary()}>生成表达偏好</Button>
          {!overview.summaryAvailable && <span className="self-center text-xs text-[var(--color-fg-subtle)]">需要至少 10 条有效证据，来自 5 次不同听写</span>}
        </div>
        {rules.length > 0 && (
          <div className="flex flex-col divide-y divide-[var(--color-line)] border-y border-[var(--color-line)]">
            {rules.map((rule) => (
              <div key={rule.id} className="flex flex-wrap items-center gap-3 py-3 text-sm">
                <span className="min-w-[220px] flex-1"><span className="text-[var(--color-fg-subtle)]">{rule.beforeText}</span> → {rule.afterText}</span>
                <span className="text-xs text-[var(--color-fg-subtle)]">{rule.scope === "global" ? "全局" : rule.appName || "当前应用"} · {rule.evidenceCount} 次证据 · {rule.status === "active" ? "生效中" : rule.status === "disabled" ? "已停用" : "候选"}</span>
                {rule.hotwordSuggested && <Button size="sm" onClick={() => void acceptHotwordSuggestion(rule)}>加入热词</Button>}
                {rule.status === "active" && <Button size="sm" onClick={() => void changeRuleScope(rule)}><Globe2 className="h-3.5 w-3.5" aria-hidden />{rule.scope === "global" ? "限制应用" : "设为全局"}</Button>}
                {rule.status !== "candidate" && <Button size="sm" onClick={() => void toggleRule(rule)}>{rule.status === "disabled" ? "启用" : "停用"}</Button>}
                <Button size="sm" variant="dangerHover" onClick={() => void removeRule(rule)}><Trash2 className="h-3.5 w-3.5" aria-hidden />删除</Button>
              </div>
            ))}
          </div>
        )}
      </SettingsSection>
      <SettingsSection title="查找记录">
        <p className="text-xs text-[var(--color-fg-subtle)]">按正文、应用、结果和任务类型筛选。</p>
        <div className="grid gap-3 md:grid-cols-[minmax(260px,1fr)_180px_180px]">
          <label className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--color-fg-faint)]" aria-hidden />
            <Input className="pl-10" value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索历史" aria-label="搜索历史" />
          </label>
          <Select value={status} onChange={(event) => setStatus(event.target.value)} aria-label="结果状态">
            <option value="">全部状态</option><option value="recognized">原文已保存</option><option value="processed">结果已保存，待输入</option><option value="succeeded">已完成</option><option value="failed">失败</option><option value="cancelled">已取消</option>
          </Select>
          <Select value={taskKind} onChange={(event) => setTaskKind(event.target.value)} aria-label="任务类型">
            <option value="">全部类型</option><option value="dictation">听写</option><option value="translateSpeech">语音翻译</option><option value="editSelection">选区编辑</option><option value="ask">语音问答</option>
          </Select>
        </div>
      </SettingsSection>

      <section aria-busy={loading} className="flex flex-col gap-3">
        {page.items.map((entry) => (
          <article key={entry.id} className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
            <div className="mb-3 flex flex-wrap items-center gap-2 text-xs text-[var(--color-fg-subtle)]">
              <span>{taskLabel(entry.taskKind)}</span><span>·</span><span>{statusLabel(entry.status)}</span>
              {entry.smartProcessingApplied && <><span>·</span><span>经过智能处理</span></>}
              {entry.finalTextConfidence && <><span>·</span><span>{finalTextLabel(entry)}（{finalTextSourceLabel(entry.finalTextSource)}）</span></>}
              {entry.correctionKind && <><span>·</span><span>{({ lexical: "词语纠错", punctuation: "标点", format: "格式", style: "表达调整", rewrite: "大幅改写", sensitive: "敏感内容", unknown: "未分类" } as const)[entry.correctionKind]}</span></>}
              {entry.appName && <><span>·</span><span>{entry.appName}</span></>}
              <span className="ml-auto">{new Date(entry.createdAt * 1000).toLocaleString()}</span>
            </div>
            {editing === entry.id ? (
              <Textarea value={draft} onChange={(event) => setDraft(event.target.value)} aria-label="修正结果" autoFocus />
            ) : (
              <div>
                <p className="whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-fg)]">{primaryText(entry)}</p>
                {entry.finalText && entry.diffSegments.length > 0 && (
                  <div className="mt-3 rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)] p-3 text-sm leading-6" aria-label="最终草稿差异">
                    {entry.diffSegments.map((segment, index) => (
                      <span
                        key={`${segment.kind}-${index}`}
                        className={segment.kind === "delete"
                          ? "bg-[var(--color-err)]/15 text-[var(--color-err)] line-through"
                          : segment.kind === "insert"
                            ? "bg-[var(--color-ok)]/15 text-[var(--color-ok)]"
                            : "text-[var(--color-fg-subtle)]"}
                      >{segment.text}</span>
                    ))}
                  </div>
                )}
              </div>
            )}
            {entry.error && <p className="mt-2 text-xs text-[var(--color-err)]">{entry.error}</p>}
            {entry.finalText && entry.outputText && entry.finalText !== entry.outputText && (
              <details className="mt-3 text-sm text-[var(--color-fg-subtle)]">
                <summary className="cursor-pointer focus-visible:outline focus-visible:outline-2">系统输出</summary>
                <p className="mt-2 whitespace-pre-wrap break-words leading-6">{entry.outputText}</p>
              </details>
            )}
            {entry.sourceText && entry.sourceText !== primaryText(entry) && (
              <details className="mt-3 text-sm text-[var(--color-fg-subtle)]">
                <summary className="cursor-pointer focus-visible:outline focus-visible:outline-2">{entry.taskKind === "dictation" ? "识别原文" : "原始内容"}</summary>
                <p className="mt-2 whitespace-pre-wrap break-words leading-6">{entry.sourceText}</p>
                <Button size="sm" className="mt-2" onClick={() => void copy(entry.sourceText)}>复制原文</Button>
              </details>
            )}
            <div className="mt-4 flex flex-wrap gap-2">
              <Button size="sm" onClick={() => void copy(primaryText(entry))}><Clipboard className="h-3.5 w-3.5" aria-hidden />复制</Button>
              <Button size="sm" disabled={!entry.outputText || entry.status === "recognized" || entry.status === "processed"} onClick={() => void retry(entry.id)}><RefreshCw className="h-3.5 w-3.5" aria-hidden />重试注入</Button>
              {editing === entry.id ? <>
                <Button size="sm" variant="primary" onClick={() => void save(entry)}><Check className="h-3.5 w-3.5" aria-hidden />保存</Button>
                <Button size="sm" onClick={() => setEditing(null)}><X className="h-3.5 w-3.5" aria-hidden />取消</Button>
              </> : <Button size="sm" disabled={entry.status === "recognized" || entry.status === "processed"} onClick={() => { setEditing(entry.id); setDraft(primaryText(entry)); }}><Pencil className="h-3.5 w-3.5" aria-hidden />修正</Button>}
              {entry.finalTextConfidence === "medium" && <>
                <Button size="sm" variant="primary" onClick={() => void confirmObserved(entry)}><Check className="h-3.5 w-3.5" aria-hidden />确认并学习</Button>
                <Button size="sm" onClick={() => void discardObserved(entry)}><X className="h-3.5 w-3.5" aria-hidden />忽略</Button>
              </>}
              {entry.finalText && entry.learningStatus === "candidate" && <>
                <Button size="sm" variant="primary" onClick={() => void confirmLearning(entry)}><Check className="h-3.5 w-3.5" aria-hidden />确认学习</Button>
                <Button size="sm" onClick={() => void rejectLearning(entry)}><X className="h-3.5 w-3.5" aria-hidden />仅保留记录</Button>
              </>}
              {entry.finalText && entry.learningStatus === "pending" && entry.finalTextConfidence !== "medium" && (
                <Button size="sm" onClick={() => void rejectLearning(entry)}><X className="h-3.5 w-3.5" aria-hidden />不参与学习</Button>
              )}
              <Button size="sm" variant="dangerHover" onClick={() => void remove(entry.id)}><Trash2 className="h-3.5 w-3.5" aria-hidden />删除</Button>
            </div>
          </article>
        ))}
        {!loading && page.items.length === 0 && <div className="py-16 text-center text-sm text-[var(--color-fg-subtle)]">没有符合条件的记录</div>}
        <div className="flex items-center justify-between text-xs text-[var(--color-fg-subtle)]">
          <span>{range}</span><div className="flex gap-2"><Button size="sm" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}>上一页</Button><Button size="sm" disabled={offset + PAGE_SIZE >= page.total} onClick={() => setOffset(offset + PAGE_SIZE)}>下一页</Button></div>
        </div>
      </section>
      {message && <p role="status" className="text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </div>
  );
}
