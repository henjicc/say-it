import { cn } from "@/lib/cn";

/** 分组标题：蓝色竖线 + 标题 + 向右延伸的细分割线，可选右侧操作区。 */
export function SectionHeader({
  title,
  right,
  className,
}: {
  title: React.ReactNode;
  right?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <span
        className="h-4 w-1 flex-none rounded-full bg-[var(--color-accent)]"
        aria-hidden
      />
      <h2 className="flex-none text-[15px] font-semibold text-[var(--color-fg)]">{title}</h2>
      <span className="h-px flex-1 bg-[var(--color-line)]" aria-hidden />
      {right && <div className="flex-none">{right}</div>}
    </div>
  );
}
