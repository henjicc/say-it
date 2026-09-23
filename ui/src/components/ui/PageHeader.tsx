import { HelpLabel } from "./Tooltip";
import { cn } from "@/lib/cn";

/** 页头：主标题 + 描述，右侧可放主操作按钮或状态。 */
export function PageHeader({
  title,
  description,
  actions,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <header className={cn("flex items-start justify-between gap-6", className)}>
      <div className="min-w-0">
        <h1 className="text-2xl font-semibold tracking-tight text-[var(--color-fg)]"><HelpLabel content={description}>{title}</HelpLabel></h1>
      </div>
      {actions && <div className="flex flex-none items-center gap-2">{actions}</div>}
    </header>
  );
}
