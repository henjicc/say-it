import { useEffect, useRef, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input, Textarea } from "@/components/ui/Input";
import { CheckField, Field } from "@/components/ui/Field";
import { FormGrid } from "@/components/ui/FormGrid";
import { SettingsSection } from "@/components/ui/SettingsSection";
import { Checkbox } from "@/components/ui/Checkbox";
import { Switch } from "@/components/ui/Switch";
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

function newFindRule(): LocalRule {
  return {
    id: crypto.randomUUID(),
    enabled: true,
    name: "",
    pattern: "",
    flags: "gi",
    replacement: "",
    mode: "find",
    find: "",
  };
}

export function LocalRulesPanel() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const resetLocalRules = useDictPrefs((s) => s.resetLocalRules);
  const rules = prefs.localRules;
  const regexRules = rules.filter((r) => r.mode !== "find");
  const findRules = rules.filter((r) => r.mode === "find");

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
  // 正则规则统一排在查找替换规则之前；上移/下移只在正则规则内部重排。
  const moveRule = (index: number, dir: -1 | 1) => {
    const target = index + dir;
    if (target < 0 || target >= regexRules.length) return;
    const next = regexRules.slice();
    [next[index], next[target]] = [next[target], next[index]];
    patch({ localRules: [...next, ...findRules] });
  };
  const deleteRule = (id: string) => {
    patch({ localRules: rules.filter((r) => r.id !== id) });
    if (editingId === id) setEditingId(null);
  };
  const addRule = () => {
    const rule = newRule();
    patch({ localRules: [...regexRules, rule, ...findRules] });
    setEditingId(rule.id);
  };
  const addFindRule = () => {
    const rule = newFindRule();
    patch({ localRules: [...rules, rule] });
  };
  const toggleFlag = (rule: LocalRule, flag: string, on: boolean) => {
    const set = new Set(rule.flags.split(""));
    if (on) set.add(flag);
    else set.delete(flag);
    updateRule(rule.id, { flags: FLAG_OPTIONS.map((o) => o.flag).filter((f) => set.has(f)).join("") });
  };

  return (
    <div className="flex flex-col gap-8">
      <SettingsSection
        title="本地处理"
        right={<Switch
          checked={prefs.localRulesEnabled}
          onChange={(v) => patch({ localRulesEnabled: v })}
          label="启用本地快速处理"
        />}
      >
        <p className="max-w-[75ch] text-sm leading-relaxed text-[var(--color-fg-subtle)]">
          每条规则按顺序对文本做一次查找替换。点规则名展开编辑，替换内容留空即为删除。
          规则在独立线程运行并带超时保护，写错正则也不会卡住听写。
        </p>
      </SettingsSection>

      <div
        className={cn(
          "flex flex-col gap-8 transition-opacity",
          !prefs.localRulesEnabled && "pointer-events-none opacity-40",
        )}
      >
        <SettingsSection title="查找替换">
          <p className="text-xs text-[var(--color-fg-subtle)]">
            按词整体匹配：英文词紧贴中文（无空格）或被空格 / 标点围绕都能识别到。
          </p>
          <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
          {findRules.length === 0 && (
            <p className="px-3 py-2.5 text-xs text-[var(--color-fg-faint)]">暂无查找替换规则</p>
          )}
          {findRules.map((rule) => (
            <div
              key={rule.id}
              className="flex items-center gap-2 border-b border-[var(--color-line)] px-3 py-2 last:border-b-0"
            >
              <Checkbox
                checked={rule.enabled}
                onChange={(e) => updateRule(rule.id, { enabled: e.target.checked })}
                title={rule.enabled ? "已启用" : "已停用"}
              />
              <Input
                value={rule.find ?? ""}
                placeholder="查找，例如 Cloud Code"
                spellCheck={false}
                onChange={(e) => updateRule(rule.id, { find: e.target.value })}
                size="sm" className="flex-1"
              />
              <span className="shrink-0 text-[var(--color-fg-faint)]">→</span>
              <Input
                value={rule.replacement}
                placeholder="替换为，留空 = 删除"
                spellCheck={false}
                onChange={(e) => updateRule(rule.id, { replacement: e.target.value })}
                size="sm" className="flex-1"
              />
              <label
                title="忽略大小写"
                className="flex shrink-0 items-center gap-1 text-[11px] text-[var(--color-fg-subtle)]"
              >
                <Checkbox
                  size="sm"
                  checked={rule.flags?.includes("i") ?? false}
                  onChange={(e) => updateRule(rule.id, { flags: e.target.checked ? "gi" : "g" })}
                />
                Aa
              </label>
              <IconButton
                size="sm"
                variant="dangerHover"
                className="h-7 w-7 shrink-0"
                label="删除查找替换规则"
                onClick={() => deleteRule(rule.id)}
              >
                <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
              </IconButton>
            </div>
          ))}
        </div>
          <div>
          <Button size="sm" onClick={addFindRule}>
            <Plus className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
            添加查找替换
          </Button>
          </div>
        </SettingsSection>

        <SettingsSection title="正则规则">
          <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-line)] bg-[var(--color-bg)]">
          {regexRules.map((rule, i) => {
            const err = validateRule(rule.pattern, rule.flags);
            const open = editingId === rule.id;
            return (
              <div key={rule.id} className="border-b border-[var(--color-line)] last:border-b-0">
                <div className="flex items-center gap-2.5 px-3 py-2">
                  <Checkbox
                    checked={rule.enabled}
                    onChange={(e) => updateRule(rule.id, { enabled: e.target.checked })}
                    title={rule.enabled ? "已启用" : "已停用"}
                  />
                  <button
                    type="button"
                    onClick={() => setEditingId(open ? null : rule.id)}
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-2 text-left text-sm",
                      rule.enabled ? "text-[var(--color-fg)]" : "text-[var(--color-fg-subtle)]",
                    )}
                  >
                    <span className="truncate">{rule.name || "（未命名规则）"}</span>
                    {rule.builtin && (
                      <span className="shrink-0 rounded-[var(--radius-sm)] bg-[var(--color-surface-strong)] px-1.5 py-0.5 text-[10px] text-[var(--color-fg-subtle)]">
                        内置
                      </span>
                    )}
                    {err && (
                      <span className="shrink-0 text-[11px] text-[var(--color-err)]" title={err}>
                        ● 正则错误
                      </span>
                    )}
                  </button>
                  <span
                    className={cn(
                      "shrink-0 text-[var(--color-fg-faint)] transition-transform",
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
                      size="sm"
                    />
                    <div className="flex items-center gap-2">
                      <Input
                        value={rule.pattern}
                        placeholder="正则表达式，例如 (?:嗯|呃)+"
                        spellCheck={false}
                        onChange={(e) => updateRule(rule.id, { pattern: e.target.value })}
                        size="sm"
                        className={cn("flex-1 font-mono", err && "border-[var(--color-err)]")}
                      />
                      <span className="shrink-0 text-xs text-[var(--color-fg-faint)]">替换为</span>
                      <Input
                        value={rule.replacement}
                        placeholder="留空 = 删除"
                        spellCheck={false}
                        onChange={(e) => updateRule(rule.id, { replacement: e.target.value })}
                        size="sm" className="flex-1 font-mono"
                      />
                    </div>
                    {err && <p className="text-[11px] text-[var(--color-err)]">正则错误：{err}</p>}
                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
                      {FLAG_OPTIONS.map((opt) => (
                        <CheckField
                          key={opt.flag}
                          checked={rule.flags.includes(opt.flag)}
                          onChange={(v) => toggleFlag(rule, opt.flag, v)}
                          className="text-xs text-[var(--color-fg-muted)]"
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
                          disabled={i === regexRules.length - 1}
                          onClick={() => moveRule(i, 1)}
                        >
                          ↓
                        </Button>
                        <IconButton
                          size="sm"
                          variant="dangerHover"
                          className="h-7 w-7"
                          label={rule.builtin ? "内置规则不可删除（可停用）" : "删除正则规则"}
                          disabled={rule.builtin}
                          onClick={() => deleteRule(rule.id)}
                        >
                          <Trash2 className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
                        </IconButton>
                      </div>
                    </div>
                    {rule.note && <p className="text-[11px] text-[var(--color-fg-subtle)]">{rule.note}</p>}
                  </div>
                )}
              </div>
            );
          })}
        </div>

          <div className="flex items-center gap-2">
          <Button size="sm" onClick={addRule}>
            <Plus className="h-3.5 w-3.5" strokeWidth={1.8} aria-hidden />
            添加正则规则
          </Button>
          <Button size="sm" onClick={resetLocalRules}>
            恢复内置默认
          </Button>
          <span className="text-xs text-[var(--color-fg-subtle)]">恢复默认会清除自定义规则。</span>
          </div>
        </SettingsSection>

        <SettingsSection title="试运行">
          <FormGrid className="gap-y-3">
          <Field label="输入">
            <Textarea
              rows={3}
              spellCheck={false}
              value={previewIn}
              onChange={(e) => setPreviewIn(e.target.value)}
            />
          </Field>
          <Field label="输出">
            <Textarea
              rows={3}
              readOnly
              spellCheck={false}
              value={previewOut}
              className="bg-[var(--color-bg)]"
            />
          </Field>
          </FormGrid>
          {previewNote && <p className="text-[11px] text-[var(--color-err)]">{previewNote}</p>}
        </SettingsSection>
      </div>
    </div>
  );
}
