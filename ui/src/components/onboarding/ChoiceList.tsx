import { useRef } from "react";
import { cn } from "@/lib/cn";

export interface ChoiceItem<K extends string> {
  key: K;
  title: string;
  description: string;
  recommended?: boolean;
}

/**
 * 单选列表：分组面里每行一个选项，选中项在本行下方展开 `detail`。
 * 上下方向键在选项间移动并选中，只有选中项在 Tab 顺序里（单选组的标准键盘行为）。
 */
export function ChoiceList<K extends string>({
  label,
  items,
  value,
  onChange,
  detail,
}: {
  label: string;
  items: readonly ChoiceItem<K>[];
  value: K;
  onChange: (key: K) => void;
  detail?: (key: K) => React.ReactNode;
}) {
  const refs = useRef(new Map<K, HTMLDivElement>());

  const move = (from: number, delta: number) => {
    const next = items[(from + delta + items.length) % items.length];
    onChange(next.key);
    refs.current.get(next.key)?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="rounded-[var(--radius-lg)] border border-[var(--color-line)] bg-[var(--color-surface)] shadow-[var(--shadow-raise)] [&>*+*]:border-t [&>*+*]:border-[var(--color-line)]"
    >
      {items.map((item, index) => {
        const selected = item.key === value;
        return (
          <div key={item.key} className="px-3.5">
            <div
              ref={(node) => {
                if (node) refs.current.set(item.key, node);
                else refs.current.delete(item.key);
              }}
              role="radio"
              aria-checked={selected}
              tabIndex={selected ? 0 : -1}
              onClick={() => onChange(item.key)}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown" || event.key === "ArrowRight") {
                  event.preventDefault();
                  move(index, 1);
                } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
                  event.preventDefault();
                  move(index, -1);
                } else if (event.key === " ") {
                  event.preventDefault();
                  onChange(item.key);
                }
              }}
              className="flex cursor-pointer items-start gap-3 rounded-[var(--radius-sm)] py-3.5 outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-ring)]"
            >
              <span
                aria-hidden
                className={cn(
                  "mt-0.5 h-4 w-4 flex-none rounded-full border-[1.5px]",
                  selected ? "border-[5px] border-[var(--color-accent-control)]" : "border-[var(--color-line-strong)]",
                )}
              />
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2 text-[13px] text-[var(--color-fg)]">
                  {item.title}
                  {item.recommended && (
                    <span className="rounded-[var(--radius-sm)] border border-[var(--color-line-strong)] px-1.5 text-xs text-[var(--color-accent-text)]">推荐</span>
                  )}
                </span>
                <span className="mt-0.5 block text-xs text-[var(--color-fg-subtle)]">{item.description}</span>
              </span>
            </div>
            {selected && detail && <div className="pb-3.5 pl-7">{detail(item.key)}</div>}
          </div>
        );
      })}
    </div>
  );
}
