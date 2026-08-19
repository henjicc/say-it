import { useCallback, useEffect, useMemo, useState } from "react";
import { Check, Clipboard, Pencil, RefreshCw, Search, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Input, Select, Textarea } from "@/components/ui/Input";
import { PageHeader } from "@/components/ui/PageHeader";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { CMD, EVT, cmd, on, type HistoryEntry, type HistoryPage } from "@/lib/tauri";

const PAGE_SIZE = 30;

function taskLabel(kind: HistoryEntry["taskKind"]) {
  return ({ dictation: "听写", translateSpeech: "语音翻译", editSelection: "选区编辑", ask: "语音问答" } as const)[kind] || kind;
}

function statusLabel(status: HistoryEntry["status"]) {
  return ({ succeeded: "已完成", failed: "失败", cancelled: "已取消" } as const)[status] || status;
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

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setPage(await cmd<HistoryPage>(CMD.queryHistory, {
        query: { search, status, taskKind, offset, limit: PAGE_SIZE },
      }));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setLoading(false);
    }
  }, [offset, search, status, taskKind]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void on(EVT.historyChanged, () => void load()).then((value) => { unlisten = value; });
    return () => unlisten?.();
  }, [load]);
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
      await cmd(CMD.updateHistoryText, { id: entry.id, outputText: draft });
      setEditing(null);
      setMessage("已保存，并记录为一条显式纠错样本");
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
            <option value="">全部状态</option><option value="succeeded">已完成</option><option value="failed">失败</option><option value="cancelled">已取消</option>
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
              {entry.appName && <><span>·</span><span>{entry.appName}</span></>}
              <span className="ml-auto">{new Date(entry.createdAt * 1000).toLocaleString()}</span>
            </div>
            {editing === entry.id ? (
              <Textarea value={draft} onChange={(event) => setDraft(event.target.value)} aria-label="修正结果" autoFocus />
            ) : (
              <p className="whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-fg)]">{entry.outputText || entry.sourceText}</p>
            )}
            {entry.error && <p className="mt-2 text-xs text-[var(--color-err)]">{entry.error}</p>}
            <div className="mt-4 flex flex-wrap gap-2">
              <Button size="sm" onClick={() => void copy(entry.outputText || entry.sourceText)}><Clipboard className="h-3.5 w-3.5" aria-hidden />复制</Button>
              <Button size="sm" disabled={!entry.outputText} onClick={() => void retry(entry.id)}><RefreshCw className="h-3.5 w-3.5" aria-hidden />重试注入</Button>
              {editing === entry.id ? <>
                <Button size="sm" variant="primary" onClick={() => void save(entry)}><Check className="h-3.5 w-3.5" aria-hidden />保存</Button>
                <Button size="sm" onClick={() => setEditing(null)}><X className="h-3.5 w-3.5" aria-hidden />取消</Button>
              </> : <Button size="sm" onClick={() => { setEditing(entry.id); setDraft(entry.outputText || entry.sourceText); }}><Pencil className="h-3.5 w-3.5" aria-hidden />修正</Button>}
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
