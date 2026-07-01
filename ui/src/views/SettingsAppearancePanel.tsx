import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card, CardDescription, CardTitle } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/cn";
import { defaultAccentTheme, useThemeStore } from "@/store/useThemeStore";

function isHexColor(value: string) {
  return /^#?[0-9a-fA-F]{3}$/.test(value.trim()) || /^#?[0-9a-fA-F]{6}$/.test(value.trim());
}

function AccentColorField({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const invalid = draft.trim() !== "" && !isHexColor(draft);

  useEffect(() => {
    setDraft(value);
  }, [value]);

  const commit = () => {
    if (isHexColor(draft)) onChange(draft);
    else setDraft(value);
  };

  return (
    <div className="grid grid-cols-[2.75rem_1fr] gap-3 rounded-xl border border-white/10 bg-white/[0.035] p-3">
      <input
        type="color"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-label="强调色"
        className="h-10 w-10 cursor-pointer rounded-lg border border-white/15 bg-transparent p-0.5 [accent-color:var(--color-accent)]"
      />
      <div className="min-w-0">
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-sm font-medium text-white">强调色</p>
            <p className="mt-0.5 text-xs text-white/42">按钮、选中项、焦点与滑块颜色</p>
          </div>
          <span className="shrink-0 font-mono text-xs text-white/48">{value}</span>
        </div>
        <Input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
          className={cn("mt-2 h-9 font-mono", invalid && "border-[#ff4d4f]/60")}
          spellCheck={false}
        />
        {invalid && <p className="mt-1 text-xs text-[#ff8589]">请输入 3 或 6 位 Hex 颜色。</p>}
      </div>
    </div>
  );
}

export function SettingsAppearancePanel() {
  const theme = useThemeStore((s) => s.theme);
  const patch = useThemeStore((s) => s.patch);
  const reset = useThemeStore((s) => s.reset);

  return (
    <Card>
      <CardTitle>外观</CardTitle>
      <CardDescription>只保留整体色调和强调色。默认强调色为 {defaultAccentTheme.accent}。</CardDescription>

      <div className="mt-4 grid grid-cols-1 gap-3 lg:grid-cols-[1fr_18rem]">
        <div className="grid gap-3">
          <div className="rounded-xl border border-white/10 bg-white/[0.035] p-3">
            <p className="text-sm font-medium text-white">整体色调</p>
            <p className="mt-0.5 text-xs text-white/42">切换界面基础明暗。</p>
            <div className="mt-3 grid grid-cols-2 gap-1 rounded-xl border border-white/10 bg-black/20 p-1">
              {[
                { value: "dark", label: "暗色调" },
                { value: "light", label: "亮色调" },
              ].map((option) => {
                const active = theme.tone === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => patch({ tone: option.value as "dark" | "light" })}
                    className={cn(
                      "h-9 rounded-lg text-sm transition-colors",
                      active
                        ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-contrast)]"
                        : "text-white/58 hover:bg-white/[0.07] hover:text-white",
                    )}
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>
          </div>

          <AccentColorField value={theme.accent} onChange={(value) => patch({ accent: value })} />
        </div>

        <div className="rounded-2xl border border-white/10 bg-[radial-gradient(circle_at_30%_20%,color-mix(in_srgb,var(--color-accent-light)_24%,transparent),transparent_42%),linear-gradient(145deg,rgba(255,255,255,0.08),rgba(255,255,255,0.025))] p-4">
          <div className="rounded-xl border border-white/10 bg-black/30 p-3">
            <p className="text-xs text-white/45">预览</p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button size="sm" variant="primary">
                主要操作
              </Button>
              <Button size="sm">次要操作</Button>
            </div>
            <div className="mt-4 h-2 rounded-full bg-white/10">
              <div className="h-full w-2/3 rounded-full bg-[linear-gradient(90deg,var(--color-accent-dark),var(--color-accent-light))]" />
            </div>
            <div className="mt-4 flex items-center gap-2 rounded-xl bg-[color-mix(in_srgb,var(--color-accent)_16%,transparent)] px-3 py-2 text-sm ring-1 ring-[color-mix(in_srgb,var(--color-accent)_26%,transparent)]">
              <span className="h-2 w-2 rounded-full bg-[var(--color-accent-light)]" />
              <span className="text-white/86">当前选中状态</span>
            </div>
          </div>

          <Button className="mt-4 w-full" size="sm" onClick={reset}>
            恢复默认
          </Button>
        </div>
      </div>
    </Card>
  );
}
