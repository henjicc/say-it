import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, Clipboard, Pencil, RefreshCw, Search, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Input, Select, Textarea } from "@/components/ui/Input";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, EVT, cmd, type HistoryEntry, type HistoryPage } from "@/lib/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";

const PAGE_SIZE = 30;

function taskLabel(kind: HistoryEntry["taskKind"]) {
  return ({ dictation: "听写", translateSpeech: "语音翻译", editSelection: "选区编辑", ask: "语音问答" } as const)[kind] || kind;
}

function statusLabel(status: HistoryEntry["status"]) {
  return ({ recognized: "原文已保存", processed: "结果已保存，待输入", succeeded: "已完成", failed: "失败", cancelled: "已取消" } as const)[status] || status;
}

function finalTextLabel(entry: HistoryEntry) {
  if (entry.finalTextConfidence === "confirmed") return "已确认并学习";
  if (entry.finalTextConfidence === "high") return "高可信 · 已学习";
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
  const loadVersion = useRef(0);

  const load = useCallback(async () => {
    const version = ++loadVersion.current;
    setLoading(true);
    try {
      const next = await cmd<HistoryPage>(CMD.queryHistory, {
        query: { search, status, taskKind, offset, limit: PAGE_SIZE },
      });
      if (version === loadVersion.current) setPage(next);
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
      <PageHeader title="历史" description="默认保留最近 30 天的本地听写与智能助手结果，不保存音频。" />
      {page.recoveryNotice && (
        <p role="alert" className="rounded-[var(--radius-lg)] border border-[var(--color-warn)]/40 bg-[var(--color-warn)]/10 px-4 py-3 text-sm text-[var(--color-fg)]">
          {page.recoveryNotice}
        </p>
      )}
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
