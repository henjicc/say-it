import { Cloud, ShieldCheck } from "lucide-react";
import type { HardwareSummary } from "@/features/models/localModelNeeds";
import { localPolishFits, type UsageMode } from "@/features/onboarding/setup";
import { cn } from "@/lib/cn";

/**
 * 第 2 步 · 选择使用方式（离线模式 vs 在线服务）。
 * 严格控制每行字数，保证宽窗下左右卡片每一行单行等高、分割线完全齐平。
 */
export function ModeStep({
  mode,
  onMode,
  hardware,
}: {
  mode: UsageMode;
  onMode: (mode: UsageMode) => void;
  hardware: HardwareSummary | undefined;
}) {
  const localRecommended = localPolishFits(hardware);

  const cards = [
    {
      key: "local" as const,
      title: "离线模式",
      sub: "在自己电脑上运行",
      recommended: localRecommended,
      iconBg: "bg-[color-mix(in_srgb,var(--color-ok)_12%,transparent)] text-[var(--color-ok)]",
      icon: <ShieldCheck className="h-5 w-5" aria-hidden />,
      pros: [
        "完全免费，不限使用时长",
        "数据不出本机，断网也能用",
      ],
      notes: [
        "首次使用需下载模型文件",
        "AI 润色需独立显卡或 Apple 芯片",
      ],
    },
    {
      key: "online" as const,
      title: "在线服务",
      sub: "接入第三方云端 API",
      recommended: !localRecommended,
      iconBg: "bg-[var(--accent-soft)] text-[var(--color-accent-text)]",
      icon: <Cloud className="h-5 w-5" aria-hidden />,
      pros: [
        "识别与润色准确率更高",
        "不挑电脑配置，免下载大文件",
      ],
      notes: [
        "需填入第三方服务商 API Key",
        "本软件永久免费，仅由服务商按量计费",
      ],
    },
  ];

  return (
    <div className="space-y-6" aria-label="选择使用方式">
      <div className="space-y-2 text-center">
        <h1 className="text-[24px] font-semibold tracking-tight text-[var(--color-fg)]">选择使用方式</h1>
        <p className="text-[14px] text-[var(--color-fg-muted)]">
          以后随时可以在设置里自由切换，也可以混合使用。
        </p>
      </div>

      <div role="radiogroup" aria-label="使用方式" className="grid grid-cols-1 items-stretch gap-5 md:grid-cols-2">
        {cards.map((card) => {
          const selected = mode === card.key;
          return (
            <button
              key={card.key}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => onMode(card.key)}
              className={cn(
                "flex cursor-pointer flex-col justify-between gap-5 rounded-[var(--radius-xl)] bg-[var(--color-surface)] p-6 text-left transition-all",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]",
                selected
                  ? "border-[1.5px] border-[var(--color-accent)] shadow-[0_0_0_4px_var(--accent-soft),var(--shadow-raise)]"
                  : "border-[1.5px] border-[var(--color-line)] shadow-[var(--shadow-raise)] hover:border-[var(--color-line-strong)]",
              )}
            >
              <div className="space-y-5">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <div className={cn("flex h-11 w-11 shrink-0 items-center justify-center rounded-[14px]", card.iconBg)}>
                      {card.icon}
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="text-[17px] font-semibold text-[var(--color-fg)]">{card.title}</span>
                        {card.recommended && (
                          <span className="rounded-full bg-[var(--color-accent-control)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-accent-control-contrast)]">
                            推荐
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 text-xs text-[var(--color-fg-muted)]">{card.sub}</div>
                    </div>
                  </div>
                  <span
                    aria-hidden
                    className={cn(
                      "mt-1 flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-[1.5px]",
                      selected
                        ? "border-[var(--color-accent)] bg-[var(--color-accent)]"
                        : "border-[var(--color-line-strong)] bg-transparent",
                    )}
                  >
                    {selected && <span className="h-2 w-2 rounded-full bg-white" />}
                  </span>
                </div>

                <ul className="space-y-2.5 text-[14px] text-[var(--color-fg)]">
                  {card.pros.map((line) => (
                    <li key={line} className="flex items-center gap-2.5">
                      <span className="text-sm font-bold text-[var(--color-ok)]" aria-hidden>✓</span>
                      <span>{line}</span>
                    </li>
                  ))}
                </ul>
              </div>

              <div className="space-y-2 border-t border-[var(--color-line)] pt-4 text-xs text-[var(--color-fg-muted)]">
                {card.notes.map((line) => (
                  <div key={line} className="overflow-hidden text-ellipsis whitespace-nowrap">
                    · {line}
                  </div>
                ))}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
