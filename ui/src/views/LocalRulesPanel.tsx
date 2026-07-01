import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Input, Textarea } from "@/components/ui/Input";
import { CheckField } from "@/components/ui/Field";
import { cn } from "@/lib/cn";
import { useDictPrefs } from "@/store/useDictPrefs";
import { validateRule, type LocalRule } from "@/features/dictation/localRulesEngine";
import { runLocalRules } from "@/features/dictation/localRules";

const PREVIEW_SAMPLE = "嗯那个，我我想说的是，这个功能其实挺好的额。";

const FLAG_OPTIONS: { flag: string; label: string }[] = [
  { flag: "g", label: "全局" },
  { flag: "i", label: "忽略大小写" },
  { flag: "m", label: "多行" },
];

function newRule(): LocalRule {
  return {
    id: crypto.randomUUID(),
    enabled: true,
    name: "自定义规则",
    pattern: "",
    flags: "g",
    replacement: "",
  };
}

export function LocalRulesPanel() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const resetLocalRules = useDictPrefs((s) => s.resetLocalRules);
  const rules = prefs.localRules;

  const [editingId, setEditingId] = useState<string | null>(null);
  const [previewIn, setPreviewIn] = useState(PREVIEW_SAMPLE);
  const [previewOut, setPreviewOut] = useState("");
  const [previewNote, setPreviewNote] = useState("");
  const debounce = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 试运行：防抖后在 worker 里跑当前规则，展示处理结果。
  useEffect(() => {
    if (debounce.current) clearTimeout(debounce.current);
    debounce.current = setTimeout(async () => {
      const res = await runLocalRules(previewIn, rules);
      setPreviewOut(res.text);
      setPreviewNote(
        res.timedOut
          ? "处理超时，已回退原文（检查是否有灾难性回溯的正则）"
          : res.error
            ? `出错：${res.error}`
            : "",
      );
    }, 250);
    return () => {
      if (debounce.current) clearTimeout(debounce.current);
    };
  }, [previewIn, rules]);

  const updateRule = (id: string, partial: Partial<LocalRule>) => {
    patch({ localRules: rules.map((r) => (r.id === id ? { ...r, ...partial } : r)) });
  };
  const moveRule = (index: number, dir: -1 | 1) => {
    const target = index + dir;
    if (target < 0 || target >= rules.length) return;
    const next = rules.slice();
    [next[index], next[target]] = [next[target], next[index]];
    patch({ localRules: next });
  };
  const deleteRule = (id: string) => {
    patch({ localRules: rules.filter((r) => r.id !== id) });
    if (editingId === id) setEditingId(null);
  };
  const addRule = () => {
    const rule = newRule();
    patch({ localRules: [...rules, rule] });
    setEditingId(rule.id);
  };
  const toggleFlag = (rule: LocalRule, flag: string, on: boolean) => {
    const set = new Set(rule.flags.split(""));
    if (on) set.add(flag);
    else set.delete(flag);
    updateRule(rule.id, { flags: FLAG_OPTIONS.map((o) => o.flag).filter((f) => set.has(f)).join("") });
  };

  return (
    <div className="mt-4 rounded-xl border border-white/10 bg-white/[0.03] p-4">
      <CheckField
        checked={prefs.localRulesEnabled}
        onChange={(v) => patch({ localRulesEnabled: v })}
      >
        启用本地快速处理
      </CheckField>
      <p className="mt-1.5 text-xs text-white/40">
        每条规则按顺序对文本做一次查找替换。点规则名展开编辑，替换内容留空即为删除。
        规则在独立线程运行并带超时保护，写错正则也不会卡住听写。
      </p>

      <div
        className={cn(
          "mt-3 transition-opacity",
          !prefs.localRulesEnabled && "pointer-events-none opacity-40",
        )}
      >
        <div className="overflow-hidden rounded-lg border border-white/10 bg-black/20">
          {rules.map((rule, i) => {
            const err = validateRule(rule.pattern, rule.flags);
            const open = editingId === rule.id;
            return (
              <div key={rule.id} className="border-b border-white/5 last:border-b-0">
                <div className="flex items-center gap-2.5 px-3 py-2">
                  <input
                    type="checkbox"
                    checked={rule.enabled}
                    onChange={(e) => updateRule(rule.id, { enabled: e.target.checked })}
                    className="h-4 w-4 shrink-0 [accent-color:var(--color-accent)]"
                    title={rule.enabled ? "已启用" : "已停用"}
                  />
                  <button
                    type="button"
                    onClick={() => setEditingId(open ? null : rule.id)}
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-2 text-left text-sm",
                      rule.enabled ? "text-white/85" : "text-white/40",
                    )}
                  >
                    <span className="truncate">{rule.name || "（未命名规则）"}</span>
                    {rule.builtin && (
                      <span className="shrink-0 rounded bg-white/10 px-1.5 py-0.5 text-[10px] text-white/45">
                        内置
                      </span>
                    )}
                    {err && (
                      <span className="shrink-0 text-[11px] text-[#ff8589]" title={err}>
                        ● 正则错误
                      </span>
                    )}
                  </button>
                  <span
                    className={cn(
                      "shrink-0 text-white/30 transition-transform",
                      open && "rotate-90",
                    )}
                  >
                    ›
                  </span>
                </div>

                {open && (
                  <div className="flex flex-col gap-2 px-3 pb-3 pl-[34px]">
                    <Input
                      value={rule.name}
                      placeholder="规则名称"
                      onChange={(e) => updateRule(rule.id, { name: e.target.value })}
                      className="h-8 px-2.5 py-1 text-xs"
                    />
                    <div className="flex items-center gap-2">
                      <Input
                        value={rule.pattern}
                        placeholder="正则表达式，例如 (?:嗯|呃)+"
                        spellCheck={false}
                        onChange={(e) => updateRule(rule.id, { pattern: e.target.value })}
                        className={cn(
                          "h-8 flex-1 px-2.5 py-1 font-mono text-xs",
                          err && "border-[#ff4d4f]/60",
                        )}
                      />
                      <span className="shrink-0 text-xs text-white/30">替换为</span>
                      <Input
                        value={rule.replacement}
                        placeholder="留空 = 删除"
                        spellCheck={false}
                        onChange={(e) => updateRule(rule.id, { replacement: e.target.value })}
                        className="h-8 flex-1 px-2.5 py-1 font-mono text-xs"
                      />
                    </div>
                    {err && <p className="text-[11px] text-[#ff8589]">正则错误：{err}</p>}
                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
                      {FLAG_OPTIONS.map((opt) => (
                        <CheckField
                          key={opt.flag}
                          checked={rule.flags.includes(opt.flag)}
                          onChange={(v) => toggleFlag(rule, opt.flag, v)}
                          className="text-xs text-white/60"
                        >
                          {opt.label}
                        </CheckField>
                      ))}
                      <div className="ml-auto flex items-center gap-1">
                        <Button
                          size="sm"
                          className="h-7 w-7 px-0"
                          title="上移"
                          disabled={i === 0}
                          onClick={() => moveRule(i, -1)}
                        >
                          ↑
                        </Button>
                        <Button
                          size="sm"
                          className="h-7 w-7 px-0"
                          title="下移"
                          disabled={i === rules.length - 1}
                          onClick={() => moveRule(i, 1)}
                        >
                          ↓
                        </Button>
                        <Button
                          size="sm"
                          variant="danger"
                          className="h-7 px-2.5"
                          title={rule.builtin ? "内置规则不可删除（可停用）" : "删除"}
                          disabled={rule.builtin}
                          onClick={() => deleteRule(rule.id)}
                        >
                          删除
                        </Button>
                      </div>
                    </div>
                    {rule.note && <p className="text-[11px] text-white/35">{rule.note}</p>}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        <div className="mt-2.5 flex items-center gap-2">
          <Button size="sm" onClick={addRule}>
            + 添加规则
          </Button>
          <Button size="sm" onClick={resetLocalRules}>
            恢复内置默认
          </Button>
          <span className="text-xs text-white/35">恢复默认会清除自定义规则。</span>
        </div>

        <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-white/60">试运行 · 输入</span>
            <Textarea
              rows={3}
              spellCheck={false}
              value={previewIn}
              onChange={(e) => setPreviewIn(e.target.value)}
            />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-white/60">试运行 · 输出</span>
            <Textarea
              rows={3}
              readOnly
              spellCheck={false}
              value={previewOut}
              className="bg-white/[0.02]"
            />
          </label>
        </div>
        {previewNote && <p className="mt-1 text-[11px] text-[#ff8589]">{previewNote}</p>}
      </div>
    </div>
  );
}
